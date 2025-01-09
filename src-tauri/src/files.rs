//! This module deals with the file itself and its information.

use std::{path::Path, time::{SystemTime, UNIX_EPOCH}};
use crate::{config, storage::DownloadRecord};

/// Enums representing the possible download statuses.
#[derive(Debug, Clone)]
pub enum DownloadStatus {
    Pending,
    InProgress,
    Failed,
    Finished,
    Cancelled,
}

impl DownloadStatus {
    /// Converts a `DownloadStatus` enum into a string for storing in the database.
    ///
    /// # Example
    /// ```rust 
    /// let status = files::DownloadStatus::Pending;
    /// let status_str: String = status::to_string();
    /// ```
    pub fn to_string(&self) -> String {
        match self {
            DownloadStatus::Pending => String::from("Pending"),
            DownloadStatus::InProgress => String::from("InProgress"),
            DownloadStatus::Failed => String::from("Failed"),
            DownloadStatus::Finished => String::from("Finished"),
            DownloadStatus::Cancelled => String::from("Cancelled"),
        }
    }

    /// Converts a string into an instance of `DownloadStatus` enum. This is mainly used when the
    /// data is read from the database.
    ///
    /// # Example
    /// ```rust 
    /// let status_str: &str = "Pending";
    /// let status = files::DownloadStatus::from_string(status);
    /// ```
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

/// This defines the supported file types. This is to help in organising the files in the download
/// directory.
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
    /// Converts an instance `FileType` into a string. Usually for storing in the database or
    /// making the directory path for downloading the file.
    ///
    /// # Example
    /// ```rust 
    /// let file_type = files::FileType::Compressed;
    /// let file_type_str: String = file_type::to_string();
    /// ```
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

    /// Converts a string into an instance of `FileType` usually from the database.
    ///
    /// # Example
    /// ```rust 
    /// let file_type_str: &str = "Compressed";
    /// let file_type = files::FileType::from_string(file_type_str);
    /// ```
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
    /// Implementing a `From` trait to convert an instance of `DownloadRecord` to an instance of
    /// `File`
    ///
    /// # Example
    /// ```rust 
    /// let download_record = storage::DownloadRecord::default();
    /// let file = files::File::from(download_record);
    /// ```
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

/// The struct representing details about a file.
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
    pub download_status: DownloadStatus
}


/// This function gets the type of a file based on its extension.
/// For example, a .csv is a Document whereas a .mp4 is a Videos
/// 
/// # Arguments
/// - `extension`: The file extension.
///
/// # Returns
/// This function returns an instance of `FileType` corresponding to the correct file type based on
/// the file extension.
///
/// # Example 
/// ```rust 
/// let file_type = get_file_type("csv");
/// ```
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

/// This function gets the destination of the file to be downloaded. It combines:
/// - the download directory
/// - the file type
/// - the file name.
/// This way, it organises similar files into the same directory e.g ~/Downloads/Yad/Documents.
///
/// # Arguments
/// - `file_name`: The name of the file being downloaded.
/// - `config`: The application configs.
/// - `file_type`: The file type.
///
/// # Returns
/// This function returns two strings
/// - `dir`: The directory where the file will be saved into.
/// - `path`: The full path of where the file will be saved to.
///
/// # Example
/// ```rust
///  let cfg = configs::Config::default();
///  let file = "some_file.csv";
///  // get the extension of the file.
///  let extension = get_extension(&file);
///  let file_type = get_file_type(extension);
///  let (dir, path) - get_destination_path(file, &cfg, file_type);
/// ```
fn get_destination_path(file_name: &str,cfg: &config::Config, file_type: &FileType) -> (String, String) {
    let download_dir = Path::new(&cfg.download_dir);
    let dir = download_dir.join(format!("{:?}", file_type));
    let path = dir.join(file_name);

    let dir = dir.to_str().unwrap_or("_").to_string();
    let path = path.to_str().unwrap_or("_").to_string();


    (dir, path)
}

impl File {
    /// This constructs a new file from the file url. It is responsible for calling functions that
    /// get the file type and destination path.
    pub fn new(file_url: &str, cfg : &config::Config ) -> Self {
        let file_name = file_url.split('/')
            .last()
            .unwrap_or("");

        let extension =  file_name.split('.').last().unwrap_or("_").to_string();

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
            download_start_time:now,
            download_stop_time: 0,
            download_duration: 0,
            download_status: DownloadStatus::Pending,
        }
    }
    
}


