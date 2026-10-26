//! End-to-end tests: the real manager and engine against a local HTTP server that can
//! ignore ranges, drop connections, throttle, and change the file between requests.

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use yad_core::{
    config::Paths,
    manager::Manager,
    protocol::{Download, Status},
    service::LaunchCommand,
    storage::Storage,
};

fn content(len: usize, version: u8) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 7 + version as usize * 13) % 251) as u8).collect()
}

#[derive(Default)]
struct ServerState {
    files: Mutex<HashMap<String, (Vec<u8>, String)>>, // path → (body, content-type)
    version: Mutex<String>,                           // ETag
    ignore_ranges: AtomicBool,
    /// Close the connection after this many body bytes, for the next N responses.
    drop_after: AtomicU64,
    drops_left: AtomicUsize,
    /// Microseconds to sleep per 16 KiB written.
    throttle_us: AtomicU64,
    bytes_served: AtomicU64,
    requests: AtomicUsize,
}

struct Server {
    base: String,
    state: Arc<ServerState>,
}

impl Server {
    async fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(ServerState::default());
        *state.version.lock().unwrap() = "\"v1\"".into();
        let s = state.clone();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                let s = s.clone();
                tokio::spawn(async move {
                    let _ = handle(sock, s).await;
                });
            }
        });
        Server { base, state }
    }

    fn put(&self, path: &str, body: Vec<u8>, ctype: &str) {
        self.state.files.lock().unwrap().insert(path.into(), (body, ctype.into()));
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
}

async fn handle(mut sock: TcpStream, s: Arc<ServerState>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    s.requests.fetch_add(1, Ordering::SeqCst);
    let text = String::from_utf8_lossy(&buf).to_string();
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").split('?').next().unwrap().to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();

    let file = s.files.lock().unwrap().get(&path).cloned();
    let Some((body, ctype)) = file else {
        sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
        return Ok(());
    };
    let etag = s.version.lock().unwrap().clone();
    let total = body.len() as u64;
    let mut range = None;
    if !s.ignore_ranges.load(Ordering::SeqCst) {
        if let Some(r) = headers.get("range").and_then(|r| r.strip_prefix("bytes=")) {
            let if_range_ok = headers.get("if-range").map_or(true, |v| *v == etag);
            if if_range_ok {
                let (a, b) = r.split_once('-').unwrap();
                let a: u64 = a.parse().unwrap();
                let b: u64 = if b.is_empty() { total - 1 } else { b.parse::<u64>().unwrap().min(total - 1) };
                range = Some((a, b));
            }
        }
    }
    let (status, slice, extra) = match range {
        Some((a, b)) => ("206 Partial Content", &body[a as usize..=b as usize], format!("Content-Range: bytes {a}-{b}/{total}\r\n")),
        None => ("200 OK", &body[..], String::new()),
    };
    let accept = if s.ignore_ranges.load(Ordering::SeqCst) { "" } else { "Accept-Ranges: bytes\r\n" };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: {ctype}\r\nETag: {etag}\r\n{accept}{extra}Connection: close\r\n\r\n",
        slice.len()
    );
    sock.write_all(head.as_bytes()).await?;
    if method == "HEAD" {
        return Ok(());
    }
    let mut limit = slice.len();
    if s.drops_left.load(Ordering::SeqCst) > 0 && slice.len() > 1 {
        s.drops_left.fetch_sub(1, Ordering::SeqCst);
        limit = (s.drop_after.load(Ordering::SeqCst) as usize).min(slice.len() - 1);
    }
    for chunk in slice[..limit].chunks(16 * 1024) {
        sock.write_all(chunk).await?;
        s.bytes_served.fetch_add(chunk.len() as u64, Ordering::SeqCst);
        let t = s.throttle_us.load(Ordering::SeqCst);
        if t > 0 {
            tokio::time::sleep(Duration::from_micros(t)).await;
        }
    }
    sock.flush().await?;
    Ok(())
}

struct Env {
    manager: Manager,
    dir: tempfile::TempDir,
}

async fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::with_config_dir(&dir.path().join("config"));
    paths.ensure().unwrap();
    let storage = Storage::open(&paths.db_path()).unwrap();
    let manager = Manager::new(paths, storage, LaunchCommand::current(&["run"]));
    manager.start();
    let dl = dir.path().join("downloads");
    manager
        .handle(
            "setSettings",
            json!({"downloadDir": dl.to_string_lossy(), "notifications": false, "sortByType": false, "retries": 3}),
            0,
        )
        .await
        .unwrap();
    Env { manager, dir }
}

impl Env {
    async fn add(&self, params: Value) -> Download {
        serde_json::from_value(self.manager.handle("add", params, 0).await.unwrap()).unwrap()
    }

    async fn get(&self, id: i64) -> Download {
        serde_json::from_value(self.manager.handle("get", json!({ "id": id }), 0).await.unwrap()).unwrap()
    }

    async fn wait_for(&self, id: i64, want: Status, secs: u64) -> Download {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let d = self.get(id).await;
            if d.status == want {
                return d;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("timed out waiting for {want:?}; last state {d:?}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn downloads(&self) -> PathBuf {
        self.dir.path().join("downloads")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_large_file_in_parallel_segments() {
    let srv = Server::start().await;
    let body = content(9 * 1024 * 1024 + 123, 1);
    srv.put("/big.bin", body.clone(), "application/octet-stream");
    let env = env().await;
    let d = env.add(json!({"url": srv.url("/big.bin")})).await;
    let done = env.wait_for(d.id, Status::Completed, 30).await;
    assert_eq!(done.file_name, "big.bin");
    assert!(done.resumable);
    assert_eq!(std::fs::read(&done.destination_path).unwrap(), body);
    assert!(!yad_core::files::part_path(Path::new(&done.destination_path)).exists());
    // 9 segments of ≥1 MiB plus probing; never one request per MiB-chunk-per-retry like 1.x.
    assert!(srv.state.requests.load(Ordering::SeqCst) < 30, "{} requests", srv.state.requests.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume_continue_from_saved_position() {
    let srv = Server::start().await;
    let body = content(6 * 1024 * 1024, 2);
    srv.put("/slow.bin", body.clone(), "application/octet-stream");
    srv.state.throttle_us.store(4000, Ordering::SeqCst); // ~4 MB/s per connection
    let env = env().await;
    env.manager.handle("setSettings", json!({"connectionsPerDownload": 2}), 0).await.unwrap();
    let d = env.add(json!({"url": srv.url("/slow.bin")})).await;
    // Let it get going.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let cur = env.get(d.id).await;
        if cur.downloaded > 1024 * 1024 {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "no progress: {cur:?}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    env.manager.handle("pause", json!({"id": d.id}), 0).await.unwrap();
    let paused = env.wait_for(d.id, Status::Paused, 10).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let part = yad_core::files::part_path(Path::new(&paused.destination_path));
    assert!(part.exists(), "partial file kept while paused");
    let served_at_pause = srv.state.bytes_served.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(srv.state.bytes_served.load(Ordering::SeqCst) - served_at_pause < 200_000, "paused means no more transfer");

    srv.state.throttle_us.store(0, Ordering::SeqCst);
    env.manager.handle("resume", json!({"id": d.id}), 0).await.unwrap();
    let done = env.wait_for(d.id, Status::Completed, 30).await;
    assert_eq!(std::fs::read(&done.destination_path).unwrap(), body);
    let served = srv.state.bytes_served.load(Ordering::SeqCst);
    assert!(served < body.len() as u64 + 600_000, "resume re-downloaded too much: {served} for {}", body.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_without_ranges_uses_one_connection() {
    let srv = Server::start().await;
    let body = content(5 * 1024 * 1024, 3);
    srv.put("/plain.bin", body.clone(), "application/octet-stream");
    srv.state.ignore_ranges.store(true, Ordering::SeqCst);
    let env = env().await;
    let d = env.add(json!({"url": srv.url("/plain.bin")})).await;
    let done = env.wait_for(d.id, Status::Completed, 30).await;
    assert!(!done.resumable);
    // YAD 1.x wrote the whole body at every 1 MB offset here and produced a corrupt file.
    assert_eq!(std::fs::read(&done.destination_path).unwrap(), body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropped_connections_are_retried() {
    let srv = Server::start().await;
    let body = content(5 * 1024 * 1024, 4);
    srv.put("/flaky.bin", body.clone(), "application/octet-stream");
    let env = env().await;
    // Drop the next 4 bodies part-way (the 1-byte probe response is never dropped).
    srv.state.drop_after.store(300_000, Ordering::SeqCst);
    srv.state.drops_left.store(4, Ordering::SeqCst);
    let d = env.add(json!({"url": srv.url("/flaky.bin")})).await;
    let done = env.wait_for(d.id, Status::Completed, 60).await;
    assert_eq!(std::fs::read(&done.destination_path).unwrap(), body);
    assert!(done.retries > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn changed_file_restarts_instead_of_mixing_versions() {
    let srv = Server::start().await;
    let v1 = content(6 * 1024 * 1024, 5);
    srv.put("/changing.bin", v1, "application/octet-stream");
    srv.state.throttle_us.store(4000, Ordering::SeqCst);
    let env = env().await;
    let d = env.add(json!({"url": srv.url("/changing.bin")})).await;
    loop {
        if env.get(d.id).await.downloaded > 512 * 1024 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    env.manager.handle("pause", json!({"id": d.id}), 0).await.unwrap();
    env.wait_for(d.id, Status::Paused, 10).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // New version on the server, same size.
    let v2 = content(6 * 1024 * 1024, 6);
    srv.put("/changing.bin", v2.clone(), "application/octet-stream");
    *srv.state.version.lock().unwrap() = "\"v2\"".into();
    srv.state.throttle_us.store(0, Ordering::SeqCst);
    env.manager.handle("resume", json!({"id": d.id}), 0).await.unwrap();
    let done = env.wait_for(d.id, Status::Completed, 30).await;
    assert_eq!(std::fs::read(&done.destination_path).unwrap(), v2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_deletes_partial_file_and_remove_keeps_completed_file() {
    let srv = Server::start().await;
    srv.put("/c.bin", content(6 * 1024 * 1024, 7), "application/octet-stream");
    srv.put("/small.txt", b"hello".to_vec(), "text/plain");
    srv.state.throttle_us.store(4000, Ordering::SeqCst);
    let env = env().await;
    let d = env.add(json!({"url": srv.url("/c.bin")})).await;
    loop {
        if env.get(d.id).await.downloaded > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    env.manager.handle("cancel", json!({"id": d.id}), 0).await.unwrap();
    let c = env.wait_for(d.id, Status::Cancelled, 10).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!yad_core::files::part_path(Path::new(&c.destination_path)).exists());

    srv.state.throttle_us.store(0, Ordering::SeqCst);
    let s = env.add(json!({"url": srv.url("/small.txt"), "fileName": "greeting"})).await;
    let s = env.wait_for(s.id, Status::Completed, 10).await;
    assert_eq!(s.file_name, "greeting.txt", "extension kept on a custom name");
    env.manager.handle("remove", json!({"id": s.id}), 0).await.unwrap();
    assert!(Path::new(&s.destination_path).exists());
    let list: Vec<Download> = serde_json::from_value(env.manager.handle("list", Value::Null, 0).await.unwrap()).unwrap();
    assert!(list.iter().all(|x| x.id != s.id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_adds_return_the_running_download_and_names_dont_collide() {
    let srv = Server::start().await;
    srv.put("/same.bin", content(5 * 1024 * 1024, 8), "application/octet-stream");
    srv.put("/other/same.bin", content(1000, 9), "application/octet-stream");
    srv.state.throttle_us.store(3000, Ordering::SeqCst);
    let env = env().await;
    let a = env.add(json!({"url": srv.url("/same.bin")})).await;
    let b = env.add(json!({"url": srv.url("/same.bin")})).await;
    assert_eq!(a.id, b.id);
    let c = env.add(json!({"url": srv.url("/other/same.bin")})).await;
    assert_eq!(c.file_name, "same (1).bin");
    srv.state.throttle_us.store(0, Ordering::SeqCst);
    env.wait_for(a.id, Status::Completed, 30).await;
    env.wait_for(c.id, Status::Completed, 30).await;
    assert!(env.downloads().join("same.bin").exists());
    assert!(env.downloads().join("same (1).bin").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_with_aes_128_and_quality_choice() {
    let srv = Server::start().await;
    let key = [9u8; 16];
    srv.put("/hls/key", key.to_vec(), "application/octet-stream");
    let mut plain_all = Vec::new();
    let mut media = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n");
    for i in 0..6u64 {
        let plain = content(50_000 + i as usize * 7, i as u8);
        let mut iv = [0u8; 16];
        iv[8..].copy_from_slice(&i.to_be_bytes());
        let enc = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(&plain);
        srv.put(&format!("/hls/720/s{i}.ts"), enc, "video/mp2t");
        plain_all.extend_from_slice(&plain);
        media.push_str(&format!("#EXTINF:4.0,\ns{i}.ts\n"));
    }
    media.push_str("#EXT-X-ENDLIST\n");
    srv.put("/hls/720/index.m3u8", media.replace("\"key\"", "\"../key\"").into_bytes(), "application/vnd.apple.mpegurl");
    srv.put("/hls/1080/index.m3u8", b"#EXTM3U\n#EXTINF:4,\nnope.ts\n#EXT-X-ENDLIST\n".to_vec(), "application/vnd.apple.mpegurl");
    srv.put(
        "/hls/master.m3u8",
        b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n1080/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n720/index.m3u8\n".to_vec(),
        "application/vnd.apple.mpegurl",
    );
    let env = env().await;
    let probe = env.manager.handle("probe", json!({"url": srv.url("/hls/master.m3u8")}), 0).await.unwrap();
    assert_eq!(probe["kind"], "hls");
    let labels: Vec<&str> = probe["variants"].as_array().unwrap().iter().map(|v| v["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["1080p", "720p"]);
    let v720 = probe["variants"][1]["id"].as_str().unwrap().to_string();

    let d = env.add(json!({"url": srv.url("/hls/master.m3u8"), "variant": v720, "title": "My Talk"})).await;
    assert_eq!(d.variant.as_deref(), Some("720p"));
    assert_eq!(d.file_name, "My Talk 720p.mp4");
    let done = env.wait_for(d.id, Status::Completed, 30).await;
    let out = std::fs::read(&done.destination_path).unwrap();
    if yad_core::util::find_ffmpeg().is_none() {
        assert!(done.destination_path.ends_with("My Talk 720p.ts"), "{}", done.destination_path);
        assert_eq!(out, plain_all, "decrypted segments joined in order");
    } else {
        assert!(!out.is_empty());
    }
    assert!(!Path::new(&format!("{}.yadparts", done.destination_path)).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn errors_are_reported_plainly() {
    let srv = Server::start().await;
    let env = env().await;
    let err = env.manager.handle("add", json!({"url": "ftp://example.com/x"}), 0).await.unwrap_err();
    assert!(err.contains("http"), "{err}");
    let d = env.add(json!({"url": srv.url("/missing.zip")})).await;
    let failed = env.wait_for(d.id, Status::Failed, 10).await;
    assert!(failed.error.unwrap().contains("404"));
}

/// Real media made by ffmpeg, so the remux/merge path is exercised end to end.
/// Skipped when ffmpeg/ffprobe aren't installed.
fn ffmpeg_tools() -> Option<(PathBuf, PathBuf)> {
    let ffmpeg = yad_core::util::find_ffmpeg()?;
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    ffprobe.is_file().then_some((ffmpeg, ffprobe))
}

fn make_media(ffmpeg: &Path, dir: &Path, format: &str) {
    let out = if format == "hls" { "index.m3u8" } else { "manifest.mpd" };
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "lavfi", "-i", "testsrc=duration=6:size=320x240:rate=25"])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=6"])
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-g", "25", "-c:a", "aac"]);
    if format == "hls" {
        cmd.args(["-f", "hls", "-hls_time", "2", "-hls_playlist_type", "vod"]);
    } else {
        cmd.args(["-f", "dash", "-seg_duration", "2", "-use_template", "1", "-use_timeline", "1"]);
    }
    let status = cmd.arg(dir.join(out)).status().unwrap();
    assert!(status.success(), "ffmpeg couldn't generate test media");
}

/// (has video, has audio, duration seconds)
fn probe_media(ffprobe: &Path, file: &str) -> (bool, bool, f64) {
    let out = std::process::Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "stream=codec_type:format=duration", "-of", "json", file])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let types: Vec<&str> = v["streams"].as_array().unwrap().iter().filter_map(|s| s["codec_type"].as_str()).collect();
    let dur = v["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0);
    (types.contains(&"video"), types.contains(&"audio"), dur)
}

fn serve_dir(srv: &Server, dir: &Path, prefix: &str) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let ctype = match p.extension().and_then(|e| e.to_str()) {
            Some("m3u8") => "application/vnd.apple.mpegurl",
            Some("mpd") => "application/dash+xml",
            _ => "application/octet-stream",
        };
        srv.put(&format!("{prefix}/{name}"), std::fs::read(&p).unwrap(), ctype);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_hls_is_remuxed_to_mp4() {
    let Some((ffmpeg, ffprobe)) = ffmpeg_tools() else {
        eprintln!("skipped: ffmpeg not installed");
        return;
    };
    let media = tempfile::tempdir().unwrap();
    make_media(&ffmpeg, media.path(), "hls");
    let srv = Server::start().await;
    serve_dir(&srv, media.path(), "/hls");
    let env = env().await;
    let d = env.add(json!({"url": srv.url("/hls/index.m3u8"), "title": "Test card"})).await;
    let done = env.wait_for(d.id, Status::Completed, 60).await;
    assert!(done.destination_path.ends_with("Test card.mp4"), "{}", done.destination_path);
    assert!(done.error.is_none(), "{:?}", done.error);
    let (video, audio, dur) = probe_media(&ffprobe, &done.destination_path);
    assert!(video && audio, "video {video} audio {audio}");
    assert!((dur - 6.0).abs() < 0.5, "duration {dur}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_dash_video_and_audio_are_joined() {
    let Some((ffmpeg, ffprobe)) = ffmpeg_tools() else {
        eprintln!("skipped: ffmpeg not installed");
        return;
    };
    let media = tempfile::tempdir().unwrap();
    make_media(&ffmpeg, media.path(), "dash");
    let srv = Server::start().await;
    serve_dir(&srv, media.path(), "/dash");
    let env = env().await;
    let probe = env.manager.handle("probe", json!({"url": srv.url("/dash/manifest.mpd")}), 0).await.unwrap();
    assert_eq!(probe["kind"], "dash");
    let video = probe["variants"].as_array().unwrap().iter().find(|v| v["label"] == "240p").expect("240p variant").clone();
    let d = env.add(json!({"url": srv.url("/dash/manifest.mpd"), "variant": video["id"], "title": "Test card"})).await;
    let done = env.wait_for(d.id, Status::Completed, 60).await;
    assert!(done.destination_path.ends_with("Test card 240p.mp4"), "{}", done.destination_path);
    let (has_video, has_audio, dur) = probe_media(&ffprobe, &done.destination_path);
    assert!(has_video && has_audio, "separate DASH tracks joined: video {has_video} audio {has_audio}");
    assert!((dur - 6.0).abs() < 0.5, "duration {dur}");
}
