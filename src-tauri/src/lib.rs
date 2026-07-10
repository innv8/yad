use reqwest::Client;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Seek, SeekFrom, Write},
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::{self, Emitter};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Semaphore;

pub mod config;
pub mod files;
pub mod storage;

const CHUNK_SIZE: u64 = 1024 * 1024;
const MAX_CONCURRENT_CHUNKS: usize = 4;
const BROWSER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

fn active_downloads() -> &'static Mutex<HashMap<i64, Arc<AtomicBool>>> {
    static MAP: OnceLock<Mutex<HashMap<i64, Arc<AtomicBool>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

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
    timestamp: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadMessage<'a> {
    download_id: i64,
    message: &'a str,
    status: &'a str,
}

#[tauri::command]
async fn download(
    window: tauri::Window,
    url: String,
    file_name: Option<String>,
    destination_dir: Option<String>,
) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with("ftp://") {
        let _ = window.emit(
            "download-message",
            DownloadMessage {
                download_id: 0,
                message: "Invalid URL. Must start with http://, https://, or ftp://",
                status: "error",
            },
        );
        return Err("Invalid URL".into());
    }

    let cfg = config::Config::default();
    let client = Client::new();

    let total_size = client
        .head(&url)
        .send()
        .await
        .map_err(|e| format!("HEAD request failed: {e}"))?
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| {
            let _ = window.emit(
                "download-message",
                DownloadMessage {
                    download_id: 0,
                    message: "Server did not provide Content-Length header",
                    status: "error",
                },
            );
            "Missing Content-Length header".to_string()
        })?;

    if total_size == 0 {
        return Err("File has zero size".into());
    }

    let mut file = files::File::new(&url, &cfg);

    if let Some(custom_name) = &file_name {
        let trimmed = custom_name.trim();
        if !trimmed.is_empty() {
            let ext = Path::new(&file.file_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let final_name = if !trimmed.contains('.') && !ext.is_empty() {
                format!("{}.{}", trimmed, ext)
            } else {
                trimmed.to_string()
            };
            file.file_name = final_name;
            file.destination_path = format!("{}/{}", file.destination_dir, file.file_name);
        }
    }

    if let Some(custom_dir) = &destination_dir {
        let trimmed = custom_dir.trim();
        if !trimmed.is_empty() {
            let dir_path = Path::new(trimmed).join(&file.file_name);
            if let Some(dir) = dir_path.parent() {
                file.destination_dir = dir.to_str().unwrap_or(&file.destination_dir).to_string();
            }
            file.destination_path = dir_path.to_str().unwrap_or(&file.destination_path).to_string();
        }
    }

    let mut record = storage::search_by_url(&url, &cfg).unwrap_or_default();

    fs::create_dir_all(&file.destination_dir)
        .map_err(|e| format!("Failed to create directory: {e}"))?;

    if record.id == 0 {
        let dr = storage::DownloadRecord::from(file.clone());
        record.id = storage::insert_record(&dr, total_size, &cfg)
            .map_err(|e| format!("Failed to save download record: {e}"))?;
    } else if record.download_status == "Finished" {
        let _ = window.emit(
            "download-message",
            DownloadMessage {
                download_id: record.id,
                message: "File already downloaded",
                status: "success",
            },
        );
        return Ok(());
    }

    let _ = window.emit(
        "download-started",
        DownloadStarted {
            download_id: record.id,
            file_url: &file.file_url,
            file_name: &file.file_name,
            file_type: &file.file_type.to_string(),
            download_status: "InProgress",
        },
    );

    let d_file = File::create(&file.destination_path)
        .map_err(|e| format!("Failed to create file: {e}"))?;
    d_file
        .set_len(total_size)
        .map_err(|e| format!("Failed to allocate file: {e}"))?;
    let d_file = Arc::new(Mutex::new(d_file));

    let cancelled = Arc::new(AtomicBool::new(false));
    active_downloads()
        .lock()
        .unwrap()
        .insert(record.id, Arc::clone(&cancelled));

    let existing_chunks = storage::get_chunks_by_record(record.id, &cfg).unwrap_or_default();
    let finished: HashMap<(u64, u64), bool> = existing_chunks
        .iter()
        .filter(|c| c.status == "Finished")
        .map(|c| ((c.start, c.end), true))
        .collect();

    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for start in (0..total_size).step_by(CHUNK_SIZE as usize) {
        let end = (start + CHUNK_SIZE - 1).min(total_size - 1);
        if finished.contains_key(&(start, end)) {
            continue;
        }
        let chunk = storage::Chunk::new(record.id, start, end);
        let _ = storage::save_chunk(&chunk, &cfg);
        ranges.push((start, end));
    }

    let progress = Arc::new(Mutex::new(0u64));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadProgress>(64);
    let pw = window.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            let _ = pw.emit("download-progress", p);
        }
    });

    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS));
    let mut handles = Vec::with_capacity(ranges.len());

    for (start, end) in ranges {
        let s = Arc::clone(&sem);
        let client = client.clone();
        let d_file = Arc::clone(&d_file);
        let tx = tx.clone();
        let url = url.clone();
        let p = Arc::clone(&progress);
        let cancelled = Arc::clone(&cancelled);
        let c = config::Config::default();
        let rid = record.id;

        handles.push(tokio::spawn(async move {
            let _permit = s.acquire().await;

            if cancelled.load(Ordering::Relaxed) {
                let _ = storage::update_chunk(rid, start, "Cancelled", &c);
                return;
            }

            let result = client
                .get(&url)
                .header("Range", format!("bytes={start}-{end}"))
                .header("User-Agent", BROWSER_AGENT)
                .send()
                .await;

            match result {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => {
                        if let Ok(mut f) = d_file.lock() {
                            let _ = f.seek(SeekFrom::Start(start));
                            let _ = f.write_all(&bytes);
                        }

                        let mut prog = p.lock().unwrap();
                        *prog += bytes.len() as u64;
                        let current = *prog;
                        drop(prog);

                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let _ = tx.try_send(DownloadProgress {
                            download_id: rid,
                            downloaded: current,
                            total_size,
                            timestamp: now,
                        });

                        let _ = storage::update_chunk(rid, start, "Finished", &c);
                    }
                    Err(e) => {
                        eprintln!("Chunk {start}-{end} body failed: {e}");
                        let _ = storage::update_chunk(rid, start, "Failed", &c);
                    }
                },
                Err(e) => {
                    eprintln!("Chunk {start}-{end} request failed: {e}");
                    let _ = storage::update_chunk(rid, start, "Failed", &c);
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    drop(tx);
    let _ = progress_task.await;

    active_downloads().lock().unwrap().remove(&record.id);

    let (pending, _finished, failed) =
        storage::count_chunks(record.id, &cfg).unwrap_or_default();

    if failed > 0 || pending > 0 {
        let _ = window.emit(
            "download-message",
            DownloadMessage {
                download_id: record.id,
                message: if failed > 0 {
                    "Download completed with errors — some chunks failed"
                } else {
                    "Download incomplete — some chunks are pending"
                },
                status: "error",
            },
        );
        let _ = window
            .notification()
            .builder()
            .title("YAD — Download incomplete")
            .body(&format!("{} — {} chunks failed", file.file_name, failed))
            .show();
    } else {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ =
            storage::update_download_record(record.id, "Finished", Some(now), total_size, &cfg);

        let _ = window.emit(
            "download-message",
            DownloadMessage {
                download_id: record.id,
                message: "Download completed successfully",
                status: "success",
            },
        );
        let _ = window
            .notification()
            .builder()
            .title("YAD — Download complete")
            .body(&format!("{} downloaded successfully", file.file_name))
            .show();
    }

    Ok(())
}

#[tauri::command]
fn cancel_download(download_id: i64) -> Result<(), String> {
    let map = active_downloads().lock().unwrap();
    if let Some(cancelled) = map.get(&download_id) {
        cancelled.store(true, Ordering::Relaxed);
        let cfg = config::Config::default();
        let _ = storage::update_download_record(
            download_id,
            "Cancelled",
            None,
            0,
            &cfg,
        );
        Ok(())
    } else {
        Err("No active download found with this id".into())
    }
}

#[tauri::command]
fn delete_record(id: i64) -> Result<(), String> {
    let cfg = config::Config::default();
    storage::delete_record(id, &cfg).map_err(|e| format!("Failed to delete record: {e}"))
}

#[tauri::command]
async fn open_file(path: String) -> Result<(), String> {
    let cfg = config::Config::default();
    let os: &str = &cfg.os;
    let command = match os {
        "Windows" => "explorer",
        "Darwin" => "open",
        _ => "xdg-open",
    };

    Command::new(command)
        .arg(&path)
        .output()
        .map_err(|e| format!("Failed to open file: {e}"))?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            fetch_records,
            download,
            cancel_download,
            delete_record,
            open_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
