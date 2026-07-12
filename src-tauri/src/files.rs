use std::{path::Path, time::{SystemTime, UNIX_EPOCH}};
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
        let stop = dr.download_stop_time.unwrap_or(0);
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
            download_stop_time: stop,
            download_duration: if stop > 0 { stop - dr.download_start_time } else { 0 },
            download_status: DownloadStatus::from_string(&dr.download_status),
        }
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub id: i64,
    pub file_url: String,
    pub file_name: String,
    pub file_type: FileType,
    pub extension: String,
    pub destination_dir: String,
    pub destination_path: String,
    pub file_size: u64,
    pub download_start_time: u64,
    pub download_stop_time: u64,
    pub download_duration: u64,
    pub download_status: DownloadStatus,
}

fn get_file_type(extension: &str) -> FileType {
    match extension {
        "mp4" | "mkv" | "avi" | "mov" | "flv" | "webm" | "wmv" | "mpeg" | "mpg" | "3gp" => FileType::Videos,
        "zip" | "rar" | "7z" | "tar" | "gz" | "targz" | "tarbz2" | "tarxz" | "iso" | "xz" => FileType::Compressed,
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "alac" | "opus" | "amr" => FileType::Audio,
        "pdf" | "docx" | "doc" | "txt" | "xlsx" | "pptx" | "ppt" | "odt" | "html" | "epub" | "csv" | "xml" => FileType::Documents,
        "exe" | "msi" | "bat" | "apk" | "dmg" | "bin" | "deb" | "rpm" => FileType::Programs,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp" | "svg" | "ico" => FileType::Images,
        _ => FileType::Others,
    }
}

fn get_destination_path(file_name: &str, cfg: &config::Config, file_type: &FileType) -> (String, String) {
    let download_dir = Path::new(&cfg.download_dir);
    let dir = download_dir.join(format!("{:?}", file_type));
    let path = dir.join(file_name);
    let dir = dir.to_str().unwrap_or("_").to_string();
    let path = path.to_str().unwrap_or("_").to_string();
    (dir, path)
}

impl File {
    pub fn new(file_url: &str, cfg: &config::Config) -> Self {
        let file_name = file_url.split('/').last().unwrap_or("");
        let extension = file_name.split('.').last().unwrap_or("_").to_string();
        let file_type = get_file_type(&extension);
        let (destination_dir, destination_path) = get_destination_path(file_name, cfg, &file_type);
        let now = SystemTime::now();
        let now = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        let now = now.as_secs();

        File {
            id: 0,
            file_url: file_url.to_string(),
            file_name: file_name.to_string(),
            file_type,
            extension,
            destination_dir,
            destination_path,
            file_size: 0,
            download_start_time: now,
            download_stop_time: 0,
            download_duration: 0,
            download_status: DownloadStatus::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_cfg() -> Config {
        Config::default()
    }

    #[test]
    fn test_download_status_round_trip() {
        let variants = [
            DownloadStatus::Pending,
            DownloadStatus::InProgress,
            DownloadStatus::Failed,
            DownloadStatus::Finished,
            DownloadStatus::Cancelled,
        ];
        for v in &variants {
            let s = v.to_string();
            let back = DownloadStatus::from_string(&s);
            assert_eq!(v.to_string(), back.to_string(), "round-trip failed for {s}");
        }
    }

    #[test]
    fn test_download_status_unknown_defaults_to_pending() {
        let s = DownloadStatus::from_string("UnknownStatus");
        assert!(matches!(s, DownloadStatus::Pending));
    }

    #[test]
    fn test_file_type_round_trip() {
        let variants = [
            FileType::Compressed,
            FileType::Videos,
            FileType::Audio,
            FileType::Documents,
            FileType::Programs,
            FileType::Images,
            FileType::Others,
        ];
        for v in &variants {
            let s = v.to_string();
            let back = FileType::from_string(&s);
            assert_eq!(v.to_string(), back.to_string(), "round-trip failed for {s}");
        }
    }

    #[test]
    fn test_file_type_unknown_defaults_to_others() {
        let t = FileType::from_string("UnknownType");
        assert!(matches!(t, FileType::Others));
    }

    #[test]
    fn test_get_file_type_videos() {
        for ext in &["mp4", "mkv", "avi", "mov", "webm"] {
            assert!(matches!(get_file_type(ext), FileType::Videos), "{ext} should be Videos");
        }
    }

    #[test]
    fn test_get_file_type_compressed() {
        for ext in &["zip", "rar", "7z", "tar", "gz", "iso"] {
            assert!(matches!(get_file_type(ext), FileType::Compressed), "{ext} should be Compressed");
        }
    }

    #[test]
    fn test_get_file_type_audio() {
        for ext in &["mp3", "flac", "wav", "aac", "ogg"] {
            assert!(matches!(get_file_type(ext), FileType::Audio), "{ext} should be Audio");
        }
    }

    #[test]
    fn test_get_file_type_documents() {
        for ext in &["pdf", "docx", "txt", "csv", "html", "epub"] {
            assert!(matches!(get_file_type(ext), FileType::Documents), "{ext} should be Documents");
        }
    }

    #[test]
    fn test_get_file_type_programs() {
        for ext in &["exe", "dmg", "apk", "deb", "rpm"] {
            assert!(matches!(get_file_type(ext), FileType::Programs), "{ext} should be Programs");
        }
    }

    #[test]
    fn test_get_file_type_images() {
        for ext in &["jpg", "png", "gif", "svg", "webp"] {
            assert!(matches!(get_file_type(ext), FileType::Images), "{ext} should be Images");
        }
    }

    #[test]
    fn test_get_file_type_unknown() {
        assert!(matches!(get_file_type("xyz"), FileType::Others));
    }

    #[test]
    fn test_file_new_extracts_name() {
        let f = File::new("https://example.com/file.zip", &test_cfg());
        assert_eq!(f.file_name, "file.zip");
        assert_eq!(f.extension, "zip");
        assert!(matches!(f.file_type, FileType::Compressed));
        assert_eq!(f.file_url, "https://example.com/file.zip");
        assert_eq!(f.download_status.to_string(), "Pending");
        assert_eq!(f.id, 0);
    }

    #[test]
    fn test_file_new_video_url() {
        let f = File::new("https://example.com/movie.mp4", &test_cfg());
        assert_eq!(f.file_name, "movie.mp4");
        assert!(matches!(f.file_type, FileType::Videos));
    }

    #[test]
    fn test_file_new_url_without_extension() {
        let f = File::new("https://example.com/download", &test_cfg());
        assert_eq!(f.file_name, "download");
        assert_eq!(f.extension, "download");
    }

    #[test]
    fn test_file_new_url_with_query_params() {
        let f = File::new("https://example.com/file.pdf?token=abc", &test_cfg());
        assert_eq!(f.file_name, "file.pdf?token=abc");
    }

    #[test]
    fn test_file_new_stop_time_and_duration_are_zero() {
        let f = File::new("https://example.com/file.zip", &test_cfg());
        assert_eq!(f.download_stop_time, 0);
        assert_eq!(f.download_duration, 0);
    }

    #[test]
    fn test_file_new_start_time_is_recent() {
        let f = File::new("https://example.com/file.zip", &test_cfg());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(f.download_start_time > 0);
        assert!(f.download_start_time <= now);
        assert!(f.download_start_time > now - 10);
    }

    #[test]
    fn test_destination_path_includes_file_type_dir() {
        let cfg = test_cfg();
        let (dir, path) = get_destination_path("doc.pdf", &cfg, &FileType::Documents);
        assert!(dir.contains("Documents"), "dir should contain Documents: {dir}");
        assert!(path.ends_with("doc.pdf"), "path should end with filename: {path}");
    }

    #[test]
    fn test_destination_path_for_different_types() {
        let cfg = test_cfg();
        let (dir_vid, _) = get_destination_path("v.mp4", &cfg, &FileType::Videos);
        let (dir_aud, _) = get_destination_path("a.mp3", &cfg, &FileType::Audio);
        assert_ne!(dir_vid, dir_aud);
        assert!(dir_vid.contains("Videos"));
        assert!(dir_aud.contains("Audio"));
    }
}
