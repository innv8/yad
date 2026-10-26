//! File names, file types and destination folders.

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FileType {
    Videos,
    Audio,
    Documents,
    Programs,
    Images,
    Compressed,
    #[default]
    Others,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Videos => "Videos",
            FileType::Audio => "Audio",
            FileType::Documents => "Documents",
            FileType::Programs => "Programs",
            FileType::Images => "Images",
            FileType::Compressed => "Compressed",
            FileType::Others => "Others",
        }
    }

    pub fn parse(s: &str) -> FileType {
        match s {
            "Videos" => FileType::Videos,
            "Audio" => FileType::Audio,
            "Documents" => FileType::Documents,
            "Programs" => FileType::Programs,
            "Images" => FileType::Images,
            "Compressed" => FileType::Compressed,
            _ => FileType::Others,
        }
    }

    pub fn from_extension(ext: &str) -> FileType {
        match ext.to_ascii_lowercase().as_str() {
            "mp4" | "mkv" | "avi" | "mov" | "flv" | "webm" | "wmv" | "mpeg" | "mpg" | "3gp" | "m4v" | "ts" => {
                FileType::Videos
            }
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" => FileType::Compressed,
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "alac" | "opus" | "amr" => FileType::Audio,
            "pdf" | "docx" | "doc" | "txt" | "xlsx" | "xls" | "pptx" | "ppt" | "odt" | "ods" | "html" | "epub"
            | "csv" | "xml" | "json" | "md" | "rtf" => FileType::Documents,
            "exe" | "msi" | "bat" | "apk" | "dmg" | "pkg" | "bin" | "deb" | "rpm" | "appimage" => FileType::Programs,
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp" | "svg" | "ico" | "heic" | "avif" => {
                FileType::Images
            }
            _ => FileType::Others,
        }
    }

    pub fn from_mime(mime: &str) -> Option<FileType> {
        let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        let t = match mime.as_str() {
            m if m.starts_with("video/") => FileType::Videos,
            m if m.starts_with("audio/") => FileType::Audio,
            m if m.starts_with("image/") => FileType::Images,
            "application/pdf" | "text/csv" | "text/plain" => FileType::Documents,
            "application/zip" | "application/x-7z-compressed" | "application/x-rar-compressed"
            | "application/gzip" | "application/x-tar" | "application/x-xz" => FileType::Compressed,
            "application/x-msdownload" | "application/x-apple-diskimage"
            | "application/vnd.android.package-archive" | "application/vnd.debian.binary-package" => {
                FileType::Programs
            }
            _ => return None,
        };
        Some(t)
    }

    pub fn for_name(name: &str) -> FileType {
        FileType::from_extension(extension(name).unwrap_or(""))
    }
}

pub fn extension(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() || ext.len() > 10 {
        return None;
    }
    Some(ext)
}

/// The last path segment of a URL, without query or fragment, percent-decoded.
pub fn name_from_url(url: &str) -> String {
    let parsed = url::Url::parse(url).ok();
    let segment = parsed
        .as_ref()
        .and_then(|u| u.path_segments())
        .and_then(|mut segs| segs.rfind(|s| !s.is_empty()).map(|s| s.to_string()))
        .unwrap_or_default();
    let decoded = percent_decode_str(&segment).decode_utf8_lossy().to_string();
    let name = sanitize(&decoded);
    if name.is_empty() {
        parsed
            .and_then(|u| u.host_str().map(|h| format!("{h}-download")))
            .map(|n| sanitize(&n))
            .unwrap_or_else(|| "download".into())
    } else {
        name
    }
}

/// Reads `filename*=` (RFC 5987) or `filename=` from a Content-Disposition header.
pub fn name_from_content_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    for part in value.split(';').map(str::trim) {
        let Some((key, val)) = part.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim();
        if key == "filename*" {
            // UTF-8''file%20name.pdf
            let encoded = val.splitn(3, '\'').nth(2).unwrap_or(val);
            let decoded = percent_decode_str(encoded.trim_matches('"')).decode_utf8_lossy().to_string();
            let name = sanitize(&decoded);
            if !name.is_empty() {
                return Some(name);
            }
        } else if key == "filename" {
            let name = sanitize(val.trim_matches('"'));
            if !name.is_empty() {
                plain = Some(name);
            }
        }
    }
    plain
}

/// Makes a name safe on Windows, macOS and Linux.
pub fn sanitize(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim().trim_end_matches(['.', ' ']).to_string();
    if out == "." || out == ".." {
        out.clear();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        out = format!("_{out}");
    }
    if out.chars().count() > 200 {
        let ext = extension(&out).map(|e| e.to_string());
        let mut truncated: String = out.chars().take(180).collect();
        if let Some(ext) = ext {
            truncated = format!("{truncated}.{ext}");
        }
        out = truncated;
    }
    out
}

/// Gives a name its extension back when a user renames "report" for "report.pdf".
pub fn keep_extension(new_name: &str, old_name: &str) -> String {
    let new_name = sanitize(new_name);
    match (extension(&new_name), extension(old_name)) {
        (None, Some(ext)) => format!("{new_name}.{ext}"),
        _ => new_name,
    }
}

pub fn destination_dir(base: &Path, sort_by_type: bool, file_type: FileType) -> PathBuf {
    if sort_by_type {
        base.join(file_type.as_str())
    } else {
        base.to_path_buf()
    }
}

pub fn part_path(destination: &Path) -> PathBuf {
    let mut s = destination.as_os_str().to_owned();
    s.push(".yadpart");
    PathBuf::from(s)
}

/// Picks `name`, or `name (1).ext`, `name (2).ext`… so nothing on disk or in use is overwritten.
pub fn unique_path(dir: &Path, name: &str, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    let (stem, ext) = match extension(name) {
        Some(ext) => (&name[..name.len() - ext.len() - 1], Some(ext)),
        None => (name, None),
    };
    for n in 0..10_000 {
        let candidate = if n == 0 {
            name.to_string()
        } else {
            match ext {
                Some(ext) => format!("{stem} ({n}).{ext}"),
                None => format!("{stem} ({n})"),
            }
        };
        let path = dir.join(&candidate);
        if !path.exists() && !part_path(&path).exists() && !taken(&path) {
            return path;
        }
    }
    dir.join(format!("{stem}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_from_url_strips_query_and_decodes() {
        assert_eq!(name_from_url("https://example.com/file.pdf?token=abc#x"), "file.pdf");
        assert_eq!(name_from_url("https://example.com/a/My%20Report.pdf"), "My Report.pdf");
        assert_eq!(name_from_url("https://example.com/dir/"), "dir");
        assert_eq!(name_from_url("https://example.com/"), "example.com-download");
    }

    #[test]
    fn content_disposition_prefers_rfc5987() {
        assert_eq!(
            name_from_content_disposition("attachment; filename=\"plain.txt\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"),
            Some("résumé.pdf".into())
        );
        assert_eq!(name_from_content_disposition("attachment; filename=\"a b.zip\""), Some("a b.zip".into()));
        assert_eq!(name_from_content_disposition("inline"), None);
    }

    #[test]
    fn sanitize_removes_paths_and_reserved_names() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("a<b>c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("trailing. "), "trailing");
    }

    #[test]
    fn file_types_from_extension_and_mime() {
        assert_eq!(FileType::for_name("movie.MP4"), FileType::Videos);
        assert_eq!(FileType::for_name("archive.7z"), FileType::Compressed);
        assert_eq!(FileType::for_name("noext"), FileType::Others);
        assert_eq!(FileType::from_mime("audio/mpeg; charset=x"), Some(FileType::Audio));
        assert_eq!(FileType::from_mime("application/octet-stream"), None);
    }

    #[test]
    fn keep_extension_on_rename() {
        assert_eq!(keep_extension("report", "old.pdf"), "report.pdf");
        assert_eq!(keep_extension("report.txt", "old.pdf"), "report.txt");
    }

    #[test]
    fn unique_path_counts_up() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.zip"), b"x").unwrap();
        std::fs::write(dir.path().join("a (1).zip.yadpart"), b"x").unwrap();
        let p = unique_path(dir.path(), "a.zip", &|_| false);
        assert_eq!(p.file_name().unwrap(), "a (2).zip");
        let p = unique_path(dir.path(), "b.zip", &|p| p.ends_with("b.zip"));
        assert_eq!(p.file_name().unwrap(), "b (1).zip");
    }
}
