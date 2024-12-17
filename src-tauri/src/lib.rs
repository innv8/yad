
pub mod config;
pub mod download;
pub mod files;
pub mod storage;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn fetch_records() -> Vec<storage::DownloadRecord> {
    let cfg = config::Config::default();
    storage::read_download_records(&cfg).unwrap_or_default() 
}

#[tauri::command]
fn download(url: String) {
    println!("::: trying to download: {}", &url);
    let cfg = config::Config::default();
    let _ = match download::download(&url, &cfg) {
        Ok(f) => f,
        Err(e) => {
            println!("failed to download : {e}");
            files::File::new(&url, &cfg)
        }
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![fetch_records, download])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
