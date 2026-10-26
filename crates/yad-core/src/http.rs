//! HTTP client setup and probing.

use reqwest::{
    header::{self, HeaderMap, HeaderName, HeaderValue},
    Client, RequestBuilder, Response, StatusCode,
};
use std::time::Duration;

use crate::{
    files::{self, FileType},
    protocol::{Headers, Kind},
};

pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

pub fn client() -> Client {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(16)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("HTTP client")
}

/// Adds the browser headers stored with a download. Range and Host are ours to set.
pub fn with_headers(mut req: RequestBuilder, headers: &Headers) -> RequestBuilder {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if matches!(lower.as_str(), "range" | "host" | "content-length" | "if-range" | "accept-encoding") {
            continue;
        }
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            map.insert(name, value);
        }
    }
    req = req.headers(map);
    // Compressed bodies break byte offsets.
    req.header(header::ACCEPT_ENCODING, "identity")
}

pub fn validate_url(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|_| "That doesn't look like a link.".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        other => Err(format!("YAD can download http and https links, not {other}.")),
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProbeResult {
    pub final_url: String,
    pub size: Option<u64>,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub file_name: String,
    pub file_type: FileType,
    pub kind: Kind,
}

fn header_str(resp: &Response, name: header::HeaderName) -> Option<String> {
    resp.headers().get(name).and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string())
}

/// Parses `bytes 0-0/12345` → (start, end, total).
pub fn parse_content_range(v: &str) -> Option<(u64, u64, Option<u64>)> {
    let v = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = v.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let total = if total.trim() == "*" { None } else { Some(total.trim().parse().ok()?) };
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?, total))
}

pub fn detect_kind(url: &str, content_type: Option<&str>) -> Kind {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    let path = url::Url::parse(url).map(|u| u.path().to_ascii_lowercase()).unwrap_or_default();
    if ct.contains("mpegurl") || path.ends_with(".m3u8") {
        Kind::Hls
    } else if ct.contains("dash+xml") || path.ends_with(".mpd") {
        Kind::Dash
    } else {
        Kind::File
    }
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let m = mime.split(';').next()?.trim().to_ascii_lowercase();
    Some(match m.as_str() {
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "application/gzip" => "gz",
        "application/x-7z-compressed" => "7z",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "text/csv" => "csv",
        "text/plain" => "txt",
        _ => return None,
    })
}

/// Learns size, resumability, validators and the file name.
///
/// HEAD gives metadata cheaply; a `Range: bytes=0-0` GET is the reliable resumability test,
/// because many servers support ranges without advertising `Accept-Ranges`.
pub async fn probe(client: &Client, url: &str, headers: &Headers) -> Result<ProbeResult, String> {
    validate_url(url)?;
    let mut out = ProbeResult { final_url: url.to_string(), ..Default::default() };
    let mut disposition = None;

    if let Ok(resp) = with_headers(client.head(url), headers).send().await {
        if resp.status().is_success() {
            out.final_url = resp.url().to_string();
            out.size = header_str(&resp, header::CONTENT_LENGTH).and_then(|s| s.parse().ok()).filter(|s| *s > 0);
            out.etag = header_str(&resp, header::ETAG);
            out.last_modified = header_str(&resp, header::LAST_MODIFIED);
            out.content_type = header_str(&resp, header::CONTENT_TYPE);
            disposition = header_str(&resp, header::CONTENT_DISPOSITION);
        }
    }

    let resp = with_headers(client.get(url), headers)
        .header(header::RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| describe_error(&e))?;
    let status = resp.status();
    if !(status.is_success()) {
        return Err(describe_status(status));
    }
    out.final_url = resp.url().to_string();
    out.etag = out.etag.or_else(|| header_str(&resp, header::ETAG));
    out.last_modified = out.last_modified.or_else(|| header_str(&resp, header::LAST_MODIFIED));
    out.content_type = out.content_type.or_else(|| header_str(&resp, header::CONTENT_TYPE));
    disposition = disposition.or_else(|| header_str(&resp, header::CONTENT_DISPOSITION));
    if status == StatusCode::PARTIAL_CONTENT {
        if let Some((0, 0, total)) = header_str(&resp, header::CONTENT_RANGE).as_deref().and_then(parse_content_range) {
            out.resumable = true;
            out.size = total.or(out.size);
        }
    } else {
        out.size = out.size.or_else(|| resp.content_length().filter(|s| *s > 0));
    }
    drop(resp);

    out.kind = detect_kind(&out.final_url, out.content_type.as_deref());
    let mut name = disposition
        .as_deref()
        .and_then(files::name_from_content_disposition)
        .unwrap_or_else(|| files::name_from_url(&out.final_url));
    if files::extension(&name).is_none() {
        if let Some(ext) = out.content_type.as_deref().and_then(extension_for_mime) {
            name = format!("{name}.{ext}");
        }
    }
    out.file_type = match files::extension(&name) {
        Some(ext) => FileType::from_extension(ext),
        None => out.content_type.as_deref().and_then(FileType::from_mime).unwrap_or(FileType::Others),
    };
    out.file_name = name;
    Ok(out)
}

pub fn describe_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "The server stopped responding.".into()
    } else if e.is_connect() {
        "Couldn't connect to the server.".into()
    } else if let Some(status) = e.status() {
        describe_status(status)
    } else if e.is_body() || e.is_decode() {
        "Connection lost".into()
    } else {
        "Network error".into()
    }
}

pub fn describe_status(status: StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => format!("The server refused access ({}). The link may have expired.", status.as_u16()),
        404 | 410 => "The file isn't on the server any more (404).".into(),
        416 => "The server rejected the requested range (416).".into(),
        429 => "The server is limiting requests (429).".into(),
        s if s >= 500 => format!("The server had an error ({s})."),
        s => format!("The server answered {s}."),
    }
}

/// 4xx errors other than 408/429 will not fix themselves; don't retry them.
pub fn is_permanent(status: StatusCode) -> bool {
    status.is_client_error() && !matches!(status.as_u16(), 408 | 425 | 429)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_parsing() {
        assert_eq!(parse_content_range("bytes 0-0/12345"), Some((0, 0, Some(12345))));
        assert_eq!(parse_content_range("bytes 100-199/*"), Some((100, 199, None)));
        assert_eq!(parse_content_range("items 0-1/2"), None);
    }

    #[test]
    fn kind_detection() {
        assert_eq!(detect_kind("https://a/b/master.m3u8?x=1", None), Kind::Hls);
        assert_eq!(detect_kind("https://a/b/x", Some("application/vnd.apple.mpegurl")), Kind::Hls);
        assert_eq!(detect_kind("https://a/manifest.mpd", None), Kind::Dash);
        assert_eq!(detect_kind("https://a/video.mp4", Some("video/mp4")), Kind::File);
    }

    #[test]
    fn rejects_non_http() {
        assert!(validate_url("ftp://x/y").is_err());
        assert!(validate_url("not a url").is_err());
        assert!(validate_url(" https://x/y ").is_ok());
    }
}
