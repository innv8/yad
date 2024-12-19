use reqwest::blocking::Client;
use serde::Serialize;
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tauri::{self, Emitter, Window};

pub mod config;
pub mod download;
pub mod files;
pub mod storage;

const CHUNK_SIZE: u64 = 1024 * 8;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn fetch_records() -> Vec<storage::DownloadRecord> {
    let cfg = config::Config::default();
    storage::read_download_records(&cfg).unwrap_or_default()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadStarted<'a> {
    download_id: i64,
    file_url: &'a str,
    file_name: &'a str,
    file_type: &'a str,
    download_status: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    download_id: i64,
    total_size: u64,
    downloaded: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadFinished {
    download_id: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadMessage<'a> {
    download_id: i64,
    message: &'a str,
    status: &'a str,
}

#[tauri::command]
fn download(window: tauri::Window, url: String) -> Result<(), String> {
    let url_copy = url.clone();
    let start_time = Instant::now();
    // 1. read/write download to db and check it's id
    let cfg = config::Config::default();
    let file = files::File::new(&url, &cfg);
    let mut record = storage::search_by_url(&url, &cfg).unwrap_or_default();

    let download_id = &record.id;

    // if it does not exist, create it
    if *download_id == 0 {
        let dr = storage::DownloadRecord::from(file.clone());
        let download_id = storage::insert_record(&dr, &cfg).unwrap_or_default();
        record.id = download_id;
    }

    if record.download_status == *"Finished" {
        window
            .emit(
                "download-message",
                DownloadMessage {
                    download_id: record.id,
                    message: "File already downloaded",
                    status: "success",
                },
            )
            .unwrap();
        return Ok(());
    }

    // 2. emit the download started event
    println!("starting the download process for {}", &file.file_name);
    window
        .emit(
            "download-started",
            DownloadStarted {
                download_id: record.id,
                file_url: &file.file_url,
                file_name: &file.file_name,
                file_type: &file.file_type.to_string(),
                download_status: &record.download_status,
            },
        )
        .unwrap();

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // get size from the head request
    let response = client
        .head(&url)
        .send()
        .map_err(|e| format!("failed to read headers for file because {e}"))?;
    let total_size = response
        .headers()
        .get("content-length")
        .ok_or("Content-Length header missing")?
        .to_str()
        .map_err(|e| format!("failed to read content length because {e}"))?
        .parse::<u64>()
        .map_err(|e| format!("failed to read total size because {e}"))?;
    println!("File size for {} is {}", &file.file_name, total_size);

    let d_file = File::create(&file.destination_path)
        .map_err(|e| format!("failed to create file because {e}"))?;
    d_file
        .set_len(total_size)
        .map_err(|e| format!("failed to create blank file because {e}"))?;
    let d_file = Arc::new(Mutex::new(d_file));

    // create threads to download each chunk
    let window = Arc::new(Mutex::new(window));
    let mut handles = vec![];

    // create a channel to receive download progress 
    let (sender, receiver) = mpsc::channel::<DownloadProgress>();

    for start in (0..total_size).step_by(CHUNK_SIZE as usize) {
        let end = (start + CHUNK_SIZE - 1).min(total_size - 1);
        let client = client.clone();
        let d_file = Arc::clone(&d_file);

        let window = Arc::clone(&window);
        let url = url_copy.clone();

        let sender = sender.clone();

        let handle = thread::spawn(move || {
            let response = client
                .get(&url)
                .header("Range", format!("bytes={start}-{end}"))
                .send()
                .map_err(|e| format!("Request failed: {e}"))?
                .bytes()
                .map_err(|e| format!("Failed to read bytes because {e}"))?;
            let mut d_file = d_file
                .lock()
                .map_err(|e| format!("Mutex lock on the file failed: {e}"))?;
            d_file
                .seek(SeekFrom::Start(start))
                .map_err(|e| format!("Seek failed: {e}"))?;
            d_file
                .write_all(&response)
                .map_err(|e| format!("failed to write chunk because {e}"))?;

            sender.send(DownloadProgress {
                download_id: record.id,
                total_size,
                downloaded: end - start,
            }).map_err(|e| format!("failed to send progress: {e}"))?;

   

            Ok::<(), String>(())
        });
        handles.push(handle);
    }

    // listen for the progress events
    std::thread::spawn(move || {
        for progress in receiver {
            let window = window.lock().map_err(|e| format!("Failed to lock window: {e}")).unwrap();
            window.emit("download-progress", progress).unwrap();
        }
    });

    for handle in handles {
        handle
            .join()
            .map_err(|e| format!("Thread joining failed: {e:?}"))?;
    }

    let duration = start_time.elapsed().as_secs_f64();
    println!("finished download in: {duration}s");

   /*
    let window = window.lock().map_err(|e| format!("failed to lock window at the end: {e}"))?;
     window
         .emit(
             "download-finished",
             DownloadFinished {
                 download_id: record.id,
             },
         )
         .unwrap();
   */

    let download_stop_time = SystemTime::now();
    let download_stop_time = download_stop_time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let download_stop_time = download_stop_time.as_secs();

    storage::update_download_record(record.id, "Finished", download_stop_time, total_size, &cfg)
        .map_err(|e| format!("failed to update download record because {e}"))?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![fetch_records, download])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
