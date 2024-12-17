use std::path::Path;
use crate::{config, storage::DownloadRecord};

#[derive(Debug, Clone)]
pub enum DownloadStatus {
    Pending,
    InProgress,
    Failed,
    Finished,
    Cancelled,
}

impl DownloadStatus {
    pub fn to_string(&self) -> String {
        match self {
            DownloadStatus::Pending => String::from("Pending"),
            DownloadStatus::InProgress => String::from("InProgress"),
            DownloadStatus::Failed => String::from("Failed"),
            DownloadStatus::Finished => String::from("Finished"),
            DownloadStatus::Cancelled => String::from("Cancelled"),
        }
    }

    pub fn from_string(status: &str) -> Self {
        match status {
            "Pending" => DownloadStatus::Pending,
            "InProgress" => DownloadStatus::InProgress,
            "Failed" => DownloadStatus::Failed,
            "Finished" => DownloadStatus::Finished,
            "Cancelled" => DownloadStatus::Cancelled,
            _ => DownloadStatus::Pending,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FileType {
    Compressed,
    Videos,
    Audio,
    Documents,
    Programs,
    Images,
    Others,
}

impl FileType {
    pub fn to_string(&self) -> String {
        match self {
            FileType::Compressed => String::from("Compressed"),
            FileType::Videos => String::from("Videos"),
            FileType::Audio => String::from("Audio"), 
            FileType::Documents => String::from("Documents"),
            FileType::Programs => String::from("Programs"),
            FileType::Images => String::from("Images"),
            FileType::Others => String::from("Others"),
        }
    }

    pub fn from_string(file_type: &str) -> Self {
        match file_type {
            "Compressed" => FileType::Compressed,
            "Videos" => FileType::Videos,
            "Audio" => FileType::Audio,
            "Documents" => FileType::Documents,
            "Programs" => FileType::Programs,
            "Images" => FileType::Images,
            _ => FileType::Others,
        }
    }
}
impl From<DownloadRecord> for File {
    fn from(dr: DownloadRecord) -> Self {
        File {
            id: dr.id,
            file_url: dr.file_url,
            file_name: dr.file_name,
            file_type: FileType::from_string(&dr.file_type),
            extension: dr.extension,
            destination_dir: dr.destination_dir,
            destination_path: dr.destination_path,
            file_size: dr.file_size,
            download_start_time: dr.download_start_time,
            download_stop_time: dr.download_stop_time,
            download_duration: dr.download_stop_time - dr.download_start_time,
            download_status: DownloadStatus::from_string(&dr.download_status),
        } 
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub id: u64,
    pub file_url: String,
    pub file_name: String,
    pub file_type: FileType,
    pub extension: String,
    pub destination_dir: String,
    pub destination_path: String,
    pub file_size: u64,
    pub download_start_time: i64,
    pub download_stop_time: i64,
    pub download_duration: i64,
    pub download_status: DownloadStatus
}


fn get_file_type(extension: &str) -> FileType {
    match extension {
         "mp4" | "mkv" | "avi" | "mov" | "flv" | "webm" | "wmv" | "mpeg" | "mpg" | "3gp" => FileType::Videos,
        "zip" | "rar" | "7z" | "tar" | "gz" | "targz" | "tarbz2" | "tarxz" | "iso" | "xz" => FileType::Compressed,
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "alac" | "opus" | "amr" => FileType::Audio,
        "pdf" | "docx" | "doc" | "txt" | "xlsx" | "pptx" | "ppt" | "odt" | "html" | "epub" | "csv" | "xml" => FileType::Documents,
        "exe" | "msi" | "bat" | "apk" | "dmg"  | "bin" | "deb" | "rpm"  => FileType::Programs,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp" | "svg" | "ico" => FileType::Images,
        _ => FileType::Others,
    }
}

fn get_destination_path(file_name: &str,cfg: &config::Config, file_type: &FileType) -> (String, String) {
    let download_dir = Path::new(&cfg.download_dir);
    let dir = download_dir.join(format!("{:?}", file_type));
    let path = dir.join(file_name);

    let dir = dir.to_str().unwrap_or("_").to_string();
    let path = path.to_str().unwrap_or("_").to_string();


    (dir, path)
}

impl File {
    pub fn new(file_url: &str, cfg : &config::Config ) -> Self {
        let file_name = file_url.split('/')
            .last()
            .unwrap_or("");

        let extension =  file_name.split('.').last().unwrap_or("_").to_string();

        let file_type = get_file_type(&extension);
        let (destination_dir, destination_path) = get_destination_path(file_name, cfg, &file_type);

        File {
            id: 0,
            file_url: file_url.to_string(),
            file_name: file_name.to_string(),
            file_type,
            extension,
            destination_dir,
            destination_path,
            file_size: 0,
            download_start_time:0,
            download_stop_time: 0,
            download_duration: 0,
            download_status: DownloadStatus::Pending,
        }
    }
    
}


