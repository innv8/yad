//! Types shared by the daemon, the window and the browser extension.
//! The JSON shape is documented in `docs/ipc.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::files::FileType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Queued => "Queued",
            Status::Downloading => "Downloading",
            Status::Paused => "Paused",
            Status::Completed => "Completed",
            Status::Failed => "Failed",
            Status::Cancelled => "Cancelled",
        }
    }

    pub fn parse(s: &str) -> Status {
        match s {
            "Queued" => Status::Queued,
            "Downloading" => Status::Downloading,
            "Paused" => Status::Paused,
            "Completed" => Status::Completed,
            "Cancelled" => Status::Cancelled,
            _ => Status::Failed,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Status::Queued | Status::Downloading)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    File,
    Hls,
    Dash,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Hls => "hls",
            Kind::Dash => "dash",
        }
    }

    pub fn parse(s: &str) -> Kind {
        match s {
            "hls" => Kind::Hls,
            "dash" => Kind::Dash,
            _ => Kind::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    App,
    Extension,
    Clipboard,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::App => "app",
            Source::Extension => "extension",
            Source::Clipboard => "clipboard",
        }
    }

    pub fn parse(s: &str) -> Source {
        match s {
            "extension" => Source::Extension,
            "clipboard" => Source::Clipboard,
            _ => Source::App,
        }
    }
}

pub type Headers = BTreeMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub file_name: String,
    pub file_type: FileType,
    pub kind: Kind,
    pub destination_dir: String,
    pub destination_path: String,
    pub total_size: Option<u64>,
    pub downloaded: u64,
    pub status: Status,
    pub error: Option<String>,
    pub resumable: bool,
    pub connections: u32,
    pub speed: u64,
    pub eta: Option<u64>,
    pub retries: u32,
    pub source: Source,
    pub page_url: Option<String>,
    pub variant: Option<String>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentInfo {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadDetails {
    #[serde(flatten)]
    pub download: Download,
    pub segments: Vec<SegmentInfo>,
    pub speed_history: Vec<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub id: String,
    pub label: String,
    pub resolution: Option<String>,
    pub bandwidth: Option<u64>,
    pub codecs: Option<String>,
    pub estimated_size: Option<u64>,
    pub audio_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub url: String,
    pub file_name: String,
    pub file_type: FileType,
    pub size: Option<u64>,
    pub content_type: Option<String>,
    pub resumable: bool,
    pub kind: Kind,
    pub variants: Vec<Variant>,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowserSettings {
    pub video_button: bool,
    pub take_over: bool,
    pub types: Vec<FileType>,
    pub min_size: u64,
    pub excluded_sites: Vec<String>,
    pub send_cookies: bool,
    pub context_menu: bool,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        BrowserSettings {
            video_button: true,
            take_over: true,
            types: vec![FileType::Videos, FileType::Audio, FileType::Compressed, FileType::Programs],
            min_size: 1024 * 1024,
            excluded_sites: vec![],
            send_cookies: true,
            context_menu: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub download_dir: String,
    pub sort_by_type: bool,
    pub ask_before_download: bool,
    pub keep_running: bool,
    pub start_at_login: bool,
    pub notifications: bool,
    pub max_active: u32,
    pub connections_per_download: u32,
    pub speed_limit: Option<u64>,
    pub retries: u32,
    pub video_quality: String,
    pub merge_tracks: bool,
    pub theme: String,
    pub browser: BrowserSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: String::new(),
            sort_by_type: true,
            ask_before_download: false,
            keep_running: true,
            start_at_login: false,
            notifications: true,
            max_active: 3,
            connections_per_download: 8,
            speed_limit: None,
            retries: 5,
            video_quality: "best".into(),
            merge_tracks: true,
            theme: "system".into(),
            browser: BrowserSettings::default(),
        }
    }
}

impl Settings {
    /// Keeps numbers inside the ranges the UI offers.
    pub fn clamp(mut self) -> Self {
        self.max_active = self.max_active.clamp(1, 10);
        self.connections_per_download = self.connections_per_download.clamp(1, 16);
        self.retries = self.retries.min(20);
        if self.speed_limit == Some(0) {
            self.speed_limit = None;
        }
        if !["best", "1080", "720", "480", "audio"].contains(&self.video_quality.as_str()) {
            self.video_quality = "best".into();
        }
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        self
    }

    /// Applies a partial update. `browser` is merged key by key.
    pub fn merged(&self, patch: &Value) -> Result<Settings, String> {
        let mut current = serde_json::to_value(self).map_err(|e| e.to_string())?;
        merge_json(&mut current, patch);
        let s: Settings = serde_json::from_value(current).map_err(|e| format!("Invalid settings: {e}"))?;
        Ok(s.clamp())
    }
}

fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => merge_json(existing, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInfo {
    pub connected: bool,
    pub last_seen: Option<u64>,
    pub browser: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusInfo {
    pub version: String,
    pub pid: u32,
    pub active: u32,
    pub total_speed: u64,
    pub service_installed: bool,
    pub ffmpeg: Option<String>,
    pub extension: ExtensionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressItem {
    pub id: i64,
    pub downloaded: u64,
    pub total_size: Option<u64>,
    pub speed: u64,
    pub eta: Option<u64>,
    pub connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    Added { download: Download },
    Updated { download: Download },
    Progress { items: Vec<ProgressItem> },
    Removed { id: i64 },
    Settings { settings: Settings },
    Extension { connected: bool, browser: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub message: String,
}

/// One line on the wire: a response (`id` + `result`/`error`) or an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<ErrorBody>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub event: Option<Event>,
}

impl Message {
    pub fn ok(id: Value, result: Value) -> Self {
        Message { id: Some(id), result: Some(result), error: None, event: None }
    }
    pub fn err(id: Value, message: impl Into<String>) -> Self {
        Message { id: Some(id), result: None, error: Some(ErrorBody { message: message.into() }), event: None }
    }
    pub fn event(event: Event) -> Self {
        Message { id: None, result: None, error: None, event: Some(event) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_patch_merges_browser_keys() {
        let s = Settings::default();
        let s2 = s.merged(&json!({"maxActive": 99, "browser": {"takeOver": false}})).unwrap();
        assert_eq!(s2.max_active, 10);
        assert!(!s2.browser.take_over);
        assert!(s2.browser.video_button, "untouched browser keys keep their values");
    }

    #[test]
    fn settings_reject_wrong_types() {
        let s = Settings::default();
        assert!(s.merged(&json!({"maxActive": "lots"})).is_err());
    }

    #[test]
    fn event_serializes_with_type_tag() {
        let v = serde_json::to_value(Event::Removed { id: 4 }).unwrap();
        assert_eq!(v, json!({"type": "removed", "id": 4}));
        let v = serde_json::to_value(Event::Extension { connected: true, browser: None }).unwrap();
        assert_eq!(v["type"], "extension");
    }

    #[test]
    fn message_omits_empty_fields() {
        let s = serde_json::to_string(&Message::ok(json!(1), json!("pong"))).unwrap();
        assert_eq!(s, r#"{"id":1,"result":"pong"}"#);
    }
}
