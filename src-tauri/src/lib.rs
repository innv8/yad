use reqwest::blocking::Client;
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{Seek, SeekFrom, Write},
    process::Command,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::{self, Emitter};

pub mod config;
pub mod files;
pub mod storage;

const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB chunks
const BROWSER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

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

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    download_id: i64,
    total_size: u64,
    downloaded: u64,
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

    // 1. read/write download to db and check it's id
    let cfg = config::Config::default();

    // get file size

    let client = Client::new();
    let total_size = client
        .head(&url)
        .send()
        .map_err(|e| format!("failed to send head request: {e}"))?
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .ok_or("Content-Length header missing")?
        .to_str()
        .map_err(|e| format!("invalid content length header: {e}"))?
        .parse::<u64>()
        .map_err(|e| format!("failed to parse content length: {e}"))?;

    let file = files::File::new(&url, &cfg);
    let mut record = storage::search_by_url(&url, &cfg).unwrap_or_default();
    fs::create_dir_all(&file.destination_dir)
        .map_err(|e| format!("failed to create destination dir: {e:?}"))?;


    // if it does not exist, create it
    if record.id == 0 {
        println!("record does not exists. create it with size: {total_size}");
        let dr = storage::DownloadRecord::from(file.clone());
        record.id = match storage::insert_record(&dr, total_size, &cfg){
            Ok(id) => id,
            Err(e) => {
                panic!("failed to insert download record because {e}");
            }
        };
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

    let d_file = File::create(&file.destination_path)
        .map_err(|e| format!("failed to create file because {e}"))?;
    d_file
        .set_len(total_size)
        .map_err(|e| format!("failed to create blank file because {e}"))?;
    let d_file = Arc::new(Mutex::new(d_file));

    // Create chunks in the db.
    for start in (0..total_size).step_by(CHUNK_SIZE as usize) {
        let end = (start + CHUNK_SIZE - 1).min(total_size - 1);
        let chunk = storage::Chunk::new(record.id, start, end);
        storage::save_chunk(&chunk, &cfg).unwrap();
    }

    // create threads to download each chunk
    // create a channel to receive download progress
    let (sender, receiver) = mpsc::channel::<DownloadProgress>();
    let progress_window = window.clone();
    let progress = Arc::new(Mutex::new(0u64));

    thread::spawn(move || {
        for downloaded in receiver {
            if progress_window
                .emit("download-progress", downloaded)
                .is_err()
            {
                println!("failed to emit download progress");
            }
        }
    });

    for start in (0..total_size).step_by(CHUNK_SIZE as usize) {
        let end = (start + CHUNK_SIZE - 1).min(total_size - 1);

        let client = client.clone();
        let d_file = Arc::clone(&d_file);
        let sender = sender.clone();
        let url = url_copy.clone();
        let progress = Arc::clone(&progress);

        thread::spawn(move || {
            let cfg = config::Config::default();

            match client
                .get(&url)
                .header("Range", format!("bytes={start}-{end}"))
                .header("User-Agent", BROWSER_AGENT)
                .send()
                .and_then(|res| res.bytes())
            {
                Ok(response) => {
                    let mut d_file = d_file.lock().expect("failed to lock file");
                    d_file.seek(SeekFrom::Start(start)).expect("seek failed");
                    d_file.write_all(&response).expect("write failed");
                    let mut progress = progress.lock().unwrap();
                    // *progress += response.len() as u64;
                    *progress += end - start;

                    let _ = sender.send(DownloadProgress {
                        download_id: record.id,
                        downloaded: *progress,
                        total_size,
                    });
                    println!(">>>>>> downloaded {} of {}", *progress, total_size);

                    // update the status of the chunk
                    storage::update_chunk(record.id, start, "Finished", &cfg).unwrap();
                }
                Err(e) => {
                    println!("failed to download chunk because: {e}");
                    storage::update_chunk(record.id, start, "Failed", &cfg).unwrap();
                }
            }
        });
    }

    // because we download in threads, we will confirm the download is done once front end sends a
    // request to list downloads.

    Ok(())
}

#[tauri::command]
async fn open_file(path: String) -> Result<(), String> {
    let cfg = config::Config::default();
    let os: &str = &cfg.os;
    let command = match os {
        "Windows" => "explorer",
        "Darwin" => "open",
        _ => "nautilus",
    };

    Command::new(command)
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // when loading the application, create tables.
    let cfg = config::Config::default();
    match storage::create_tables(&cfg) {
        Ok(()) => {
            println!("created tables successfully");
        }
        Err(e) => {
            panic!("Failed to create tables because {e}");
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![fetch_records, download, open_file])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
