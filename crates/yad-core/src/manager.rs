//! The download manager: queue, scheduler, state changes, speed tracking and events.
//! Every IPC method in `docs/ipc.md` ends up here.

use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::{
    config::{Paths, VERSION},
    engine::{file, stream, Ctx, EngineError, Host, Job, Outcome, StopReason},
    files::{self, FileType},
    http,
    protocol::*,
    service::{self, LaunchCommand},
    storage::{DownloadRow, Storage},
    util::{find_ffmpeg, host_of, now_secs, RateLimiter},
};

const HISTORY: usize = 60;

#[derive(Default)]
struct Stats {
    last_bytes: Option<u64>,
    speed: u64,
    history: VecDeque<u64>,
}

struct Inner {
    paths: Paths,
    storage: Arc<Storage>,
    client: reqwest::Client,
    limiter: Arc<RateLimiter>,
    settings: RwLock<Settings>,
    jobs: Mutex<HashMap<i64, Arc<Job>>>,
    stats: Mutex<HashMap<i64, Stats>>,
    events: broadcast::Sender<Event>,
    extension: Mutex<(ExtensionInfo, HashSet<u64>)>,
    /// Serializes read-modify-write of rows.
    write: Mutex<()>,
    shutdown: CancellationToken,
    launch: LaunchCommand,
}

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AddParams {
    pub url: String,
    pub file_name: Option<String>,
    pub directory: Option<String>,
    pub headers: Option<Headers>,
    pub source: Option<Source>,
    pub page_url: Option<String>,
    pub variant: Option<String>,
    pub title: Option<String>,
    pub start: Option<bool>,
}

#[derive(Deserialize)]
struct IdParam {
    id: i64,
}

impl Manager {
    pub fn new(paths: Paths, storage: Storage, launch: LaunchCommand) -> Manager {
        let mut settings = storage.settings().ok().flatten().unwrap_or_default();
        if settings.download_dir.is_empty() {
            settings.download_dir = paths.default_download_dir.to_string_lossy().to_string();
        }
        let settings = settings.clamp();
        let _ = storage.save_settings(&settings);
        let _ = storage.requeue_interrupted();
        let (events, _) = broadcast::channel(1024);
        Manager {
            inner: Arc::new(Inner {
                paths,
                limiter: Arc::new(RateLimiter::new(settings.speed_limit)),
                storage: Arc::new(storage),
                client: http::client(),
                settings: RwLock::new(settings),
                jobs: Mutex::new(HashMap::new()),
                stats: Mutex::new(HashMap::new()),
                events,
                extension: Mutex::new((ExtensionInfo::default(), HashSet::new())),
                write: Mutex::new(()),
                shutdown: CancellationToken::new(),
                launch,
            }),
        }
    }

    /// Starts the progress ticker and the queue.
    pub fn start(&self) {
        let inner = self.inner.clone();
        tokio::spawn(async move { tick_loop(inner).await });
        self.inner.schedule();
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.inner.shutdown.clone()
    }

    pub fn settings(&self) -> Settings {
        self.inner.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn paths(&self) -> &Paths {
        &self.inner.paths
    }

    /// Stops every download so it resumes at the next start, then waits for checkpoints.
    pub async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        let jobs: Vec<Arc<Job>> = lock(&self.inner.jobs).values().cloned().collect();
        for j in &jobs {
            j.stop(StopReason::Shutdown);
        }
        for _ in 0..50 {
            if lock(&self.inner.jobs).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Handles one IPC request. `conn` identifies the connection (for extension tracking).
    pub async fn handle(&self, method: &str, params: Value, conn: u64) -> Result<Value, String> {
        log::debug!("conn {conn}: {method}");
        let i = &self.inner;
        let id = || serde_json::from_value::<IdParam>(params.clone()).map(|p| p.id).map_err(|_| "Missing id".to_string());
        let to = |v: Result<Download, String>| v.map(|d| json!(d));
        match method {
            "ping" => Ok(json!("pong")),
            "status" => Ok(json!(i.status())),
            "list" => Ok(json!(i.list()?)),
            "get" => Ok(json!(i.details(id()?)?)),
            "probe" => {
                let url = params.get("url").and_then(Value::as_str).ok_or("Missing url")?.to_string();
                let headers: Headers = params.get("headers").and_then(|h| serde_json::from_value(h.clone()).ok()).unwrap_or_default();
                Ok(json!(i.probe(&url, &headers).await?))
            }
            "add" => {
                let p: AddParams = serde_json::from_value(params).map_err(|e| format!("Invalid parameters: {e}"))?;
                to(i.add(p).await)
            }
            "pause" => to(i.pause(id()?)),
            "resume" => to(i.resume(id()?)),
            "retry" => to(i.retry(id()?).await),
            "cancel" => to(i.cancel(id()?)),
            "remove" => i.remove(id()?, false).await.map(|_| json!(true)),
            "deleteFile" => i.remove(id()?, true).await.map(|_| json!(true)),
            "rename" => {
                let name = params.get("fileName").and_then(Value::as_str).ok_or("Missing fileName")?.to_string();
                to(i.rename(id()?, &name))
            }
            "pauseAll" => {
                i.pause_all();
                Ok(json!(true))
            }
            "resumeAll" => {
                i.resume_all();
                Ok(json!(true))
            }
            "getSettings" => Ok(json!(self.settings())),
            "setSettings" => Ok(json!(i.set_settings(&params)?)),
            "installService" => {
                service::install(&i.launch, &i.paths).map_err(|e| format!("Couldn't set up start at login: {e}"))?;
                Ok(json!(i.status()))
            }
            "uninstallService" => {
                service::uninstall().map_err(|e| format!("Couldn't remove start at login: {e}"))?;
                Ok(json!(i.status()))
            }
            "hello" => {
                i.hello(conn, &params);
                Ok(json!(i.status()))
            }
            "shutdown" => {
                let me = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    me.shutdown().await;
                });
                Ok(json!(true))
            }
            other => Err(format!("Unknown method {other}")),
        }
    }

    pub fn connection_closed(&self, conn: u64) {
        self.inner.connection_closed(conn);
    }
}

impl Host for Inner {
    fn update(&self, id: i64, f: &mut dyn FnMut(&mut DownloadRow)) {
        self.modify(id, |r| f(r));
    }

    fn note_retry(&self, id: i64) {
        self.modify(id, |r| r.download.retries += 1);
    }
}

impl Inner {
    fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn modify(&self, id: i64, f: impl FnOnce(&mut DownloadRow)) -> Option<Download> {
        let row = {
            let _w = lock(&self.write);
            let mut row = self.storage.get(id).ok()??;
            f(&mut row);
            if let Err(e) = self.storage.save(&row) {
                log::error!("saving download {id}: {e}");
            }
            row
        };
        let d = self.live(row.download);
        self.emit(Event::Updated { download: d.clone() });
        Some(d)
    }

    /// Fills in what only exists while running: bytes so far, speed, connections.
    fn live(&self, mut d: Download) -> Download {
        if let Some(job) = lock(&self.jobs).get(&d.id) {
            d.downloaded = job.progress.downloaded().max(if job.progress.snapshot().is_empty() { d.downloaded } else { 0 });
            d.total_size = job.progress.total().or(d.total_size);
            d.connections = job.progress.connections.load(std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(s) = lock(&self.stats).get(&d.id) {
            d.speed = s.speed;
            d.eta = match (d.total_size, s.speed) {
                (Some(t), sp) if sp > 0 && t > d.downloaded => Some((t - d.downloaded) / sp),
                _ => None,
            };
        }
        d
    }

    fn get_row(&self, id: i64) -> Result<DownloadRow, String> {
        self.storage.get(id).map_err(|e| e.to_string())?.ok_or_else(|| "That download no longer exists.".to_string())
    }

    fn list(&self) -> Result<Vec<Download>, String> {
        Ok(self.storage.list().map_err(|e| e.to_string())?.into_iter().map(|r| self.live(r.download)).collect())
    }

    fn details(&self, id: i64) -> Result<DownloadDetails, String> {
        let row = self.get_row(id)?;
        let job = lock(&self.jobs).get(&id).cloned();
        let segments = match &job {
            Some(j) => j.progress.rows(),
            None => self.storage.segments(id).map_err(|e| e.to_string())?,
        };
        let history = lock(&self.stats).get(&id).map(|s| s.history.iter().copied().collect()).unwrap_or_default();
        let stream = row.download.kind != Kind::File;
        Ok(DownloadDetails {
            download: self.live(row.download),
            segments: segments
                .into_iter()
                .filter(|s| !stream || s.done || s.downloaded > 0)
                .map(|s| SegmentInfo { start: s.start, end: if s.end == u64::MAX { s.start + s.downloaded } else { s.end }, downloaded: s.downloaded })
                .collect(),
            speed_history: history,
            etag: row.etag,
            last_modified: row.last_modified,
        })
    }

    fn status(&self) -> StatusInfo {
        let total_speed = lock(&self.stats).values().map(|s| s.speed).sum();
        StatusInfo {
            version: VERSION.into(),
            pid: std::process::id(),
            active: lock(&self.jobs).len() as u32,
            total_speed,
            service_installed: service::is_installed(),
            ffmpeg: find_ffmpeg().map(|p| p.to_string_lossy().to_string()),
            extension: lock(&self.extension).0.clone(),
        }
    }

    async fn probe(&self, url: &str, headers: &Headers) -> Result<Probe, String> {
        let p = http::probe(&self.client, url, headers).await?;
        let mut out = Probe {
            url: p.final_url.clone(),
            file_name: p.file_name.clone(),
            file_type: p.file_type,
            size: p.size,
            content_type: p.content_type.clone(),
            resumable: p.resumable,
            kind: p.kind,
            variants: vec![],
            duration: None,
        };
        if p.kind != Kind::File {
            let info = stream::probe(&self.client, &p.final_url, headers, p.kind).await?;
            out.size = info.variants.iter().find(|v| !v.audio_only).and_then(|v| v.estimated_size);
            out.variants = info.variants;
            out.duration = info.duration;
            out.file_type = FileType::Videos;
            out.resumable = true;
            out.file_name = stream_name(None, None, &p.final_url, false);
        }
        Ok(out)
    }

    async fn add(self: &Arc<Self>, p: AddParams) -> Result<Download, String> {
        let url = http::validate_url(&p.url)?.to_string();
        // The same link already waiting or running: return it instead of a duplicate.
        if let Some(existing) = self
            .storage
            .list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|r| r.download.url == url && r.variant_id == p.variant && matches!(r.download.status, Status::Queued | Status::Downloading | Status::Paused))
        {
            return Ok(self.live(existing.download));
        }
        let headers = p.headers.clone().unwrap_or_default();
        let settings = self.settings();
        let mut kind = http::detect_kind(&url, None);
        if kind == Kind::File && p.variant.is_some() {
            // A quality was chosen, so this is a playlist without a telling extension.
            kind = match http::probe(&self.client, &url, &headers).await {
                Ok(pr) if pr.kind != Kind::File => pr.kind,
                _ => Kind::Hls,
            };
        }

        let (mut name, file_type, variant_label, estimated) = if kind == Kind::File {
            let name = match p.file_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                Some(n) => files::keep_extension(n, &files::name_from_url(&url)),
                None => files::name_from_url(&url),
            };
            let t = FileType::for_name(&name);
            (name, t, None, None)
        } else {
            let info = stream::probe(&self.client, &url, &headers, kind).await?;
            let chosen = match p.variant.as_deref() {
                Some(v) => info.variants.iter().find(|x| x.id == v).cloned(),
                None => {
                    let q = settings.video_quality.as_str();
                    info.variants
                        .iter()
                        .find(|x| match q {
                            "audio" => x.audio_only,
                            "best" => !x.audio_only,
                            q => !x.audio_only && x.label.trim_end_matches('p').parse::<u32>().is_ok_and(|h| h <= q.parse().unwrap_or(u32::MAX)),
                        })
                        .or(info.variants.first())
                        .cloned()
                }
            };
            let audio = chosen.as_ref().is_some_and(|v| v.audio_only);
            let name = stream_name(p.file_name.as_deref().or(p.title.as_deref()), chosen.as_ref().map(|v| v.label.as_str()), &url, audio);
            let t = if audio { FileType::Audio } else { FileType::Videos };
            (name, t, chosen.as_ref().map(|v| v.label.clone()), chosen.and_then(|v| v.estimated_size))
        };
        name = files::sanitize(&name);
        if name.is_empty() {
            name = "download".into();
        }

        let dir = match p.directory.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d),
            None => files::destination_dir(Path::new(&settings.download_dir), settings.sort_by_type, file_type),
        };
        let taken: HashSet<String> = self
            .storage
            .list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|r| r.download.status != Status::Completed)
            .map(|r| r.download.destination_path)
            .collect();
        let dest = files::unique_path(&dir, &name, &|p| taken.contains(&p.to_string_lossy().to_string()));
        let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or(&name).to_string();

        let start = p.start.unwrap_or(true);
        let row = DownloadRow {
            download: Download {
                id: 0,
                url: url.clone(),
                file_name: name,
                file_type,
                kind,
                destination_dir: dir.to_string_lossy().to_string(),
                destination_path: dest.to_string_lossy().to_string(),
                total_size: estimated,
                downloaded: 0,
                status: if start { Status::Queued } else { Status::Paused },
                error: None,
                resumable: false,
                connections: 0,
                speed: 0,
                eta: None,
                retries: 0,
                source: p.source.unwrap_or(Source::App),
                page_url: p.page_url.clone(),
                variant: variant_label,
                created_at: now_secs(),
                finished_at: None,
            },
            headers,
            etag: None,
            last_modified: None,
            variant_id: if kind == Kind::File { None } else { p.variant.clone() },
        };
        let id = self.storage.insert(&row).map_err(|e| e.to_string())?;
        let d = self.live(self.get_row(id)?.download);
        self.emit(Event::Added { download: d.clone() });
        self.schedule();
        Ok(d)
    }

    fn schedule(self: &Arc<Self>) {
        if self.shutdown.is_cancelled() {
            return;
        }
        let max = self.settings().max_active as usize;
        let running = lock(&self.jobs).len();
        if running >= max {
            return;
        }
        let Ok(rows) = self.storage.list() else { return };
        let mut queued: Vec<DownloadRow> = rows
            .into_iter()
            .filter(|r| r.download.status == Status::Queued && !lock(&self.jobs).contains_key(&r.download.id))
            .collect();
        queued.sort_by_key(|r| r.download.id);
        for row in queued.into_iter().take(max - running) {
            self.start_job(row);
        }
    }

    fn start_job(self: &Arc<Self>, row: DownloadRow) {
        let id = row.download.id;
        let job = Job::new(id);
        let host = host_of(&row.download.url);
        let in_use: u32 = {
            let jobs = lock(&self.jobs);
            let rows: HashMap<i64, String> = self
                .storage
                .list()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| host_of(&r.download.url).map(|h| (r.download.id, h)))
                .collect();
            jobs.iter()
                .filter(|(jid, _)| rows.get(jid) == host.as_ref())
                .map(|(_, j)| j.progress.connections.load(std::sync::atomic::Ordering::SeqCst))
                .sum()
        };
        lock(&self.jobs).insert(id, job.clone());
        lock(&self.stats).insert(id, Stats::default());
        self.modify(id, |r| {
            r.download.status = Status::Downloading;
            r.download.error = None;
        });
        let inner = self.clone();
        tokio::spawn(async move {
            let ctx = Ctx {
                client: inner.client.clone(),
                storage: inner.storage.clone(),
                limiter: inner.limiter.clone(),
                settings: inner.settings(),
                host: inner.clone() as Arc<dyn Host>,
                host_connections_in_use: in_use,
            };
            let mut result = match row.download.kind {
                Kind::File => file::run(&ctx, row.clone(), job.clone()).await,
                _ => stream::run(&ctx, row.clone(), job.clone()).await,
            };
            if let Ok(Outcome::Restream) = result {
                match inner.storage.get(id) {
                    Ok(Some(fresh)) => result = stream::run(&ctx, fresh, job.clone()).await,
                    _ => result = Ok(Outcome::Stopped),
                }
            }
            inner.finish(id, &job, result);
        });
    }

    fn finish(self: &Arc<Self>, id: i64, job: &Arc<Job>, result: Result<Outcome, EngineError>) {
        let rows = job.progress.rows();
        let downloaded = job.progress.downloaded();
        let total = job.progress.total();
        if !rows.is_empty() {
            let _ = self.storage.checkpoint(id, &rows, downloaded, total);
        }
        lock(&self.jobs).remove(&id);
        lock(&self.stats).remove(&id);
        let reason = job.reason();
        let row = self.storage.get(id).ok().flatten();

        match (result, reason) {
            (Ok(Outcome::Completed), _) => {
                let _ = self.storage.clear_segments(id);
                let d = self.modify(id, |r| {
                    r.download.status = Status::Completed;
                    r.download.finished_at = Some(now_secs());
                    if let Some(t) = r.download.total_size {
                        r.download.downloaded = t;
                    }
                });
                if let Some(d) = d {
                    self.notify("Download complete", &d.file_name);
                }
            }
            (_, StopReason::Cancel) => {
                if let Some(row) = &row {
                    remove_temp_files(&row.download);
                }
                let _ = self.storage.clear_segments(id);
            }
            (_, StopReason::Shutdown) => {
                self.modify(id, |r| r.download.status = Status::Queued);
            }
            (_, StopReason::Pause) => {
                self.modify(id, |r| {
                    if r.download.status == Status::Downloading {
                        r.download.status = Status::Paused;
                    }
                });
            }
            (Err(e), StopReason::None) => {
                let msg = e.to_string();
                log::warn!("download {id} failed: {msg}");
                let d = self.modify(id, |r| {
                    r.download.status = Status::Failed;
                    r.download.error = Some(msg.clone());
                });
                if let Some(d) = d {
                    self.notify("Download failed", &format!("{}: {msg}", d.file_name));
                }
            }
            (Ok(_), StopReason::None) => {
                self.modify(id, |r| {
                    if r.download.status == Status::Downloading {
                        r.download.status = Status::Paused;
                    }
                });
            }
        }
        self.schedule();
    }

    fn notify(&self, title: &str, body: &str) {
        if !self.settings().notifications {
            return;
        }
        let (title, body) = (title.to_string(), body.to_string());
        tokio::task::spawn_blocking(move || {
            if let Err(e) = notify_rust::Notification::new().appname("YAD").summary(&title).body(&body).show() {
                log::debug!("notification failed: {e}");
            }
        });
    }

    fn pause(self: &Arc<Self>, id: i64) -> Result<Download, String> {
        let row = self.get_row(id)?;
        match row.download.status {
            Status::Downloading | Status::Queued => {
                if let Some(job) = lock(&self.jobs).get(&id) {
                    job.stop(StopReason::Pause);
                }
                self.modify(id, |r| r.download.status = Status::Paused).ok_or("Download not found".into())
            }
            _ => Ok(self.live(row.download)),
        }
    }

    fn resume(self: &Arc<Self>, id: i64) -> Result<Download, String> {
        let row = self.get_row(id)?;
        match row.download.status {
            Status::Paused | Status::Failed | Status::Cancelled => {
                let d = self
                    .modify(id, |r| {
                        r.download.status = Status::Queued;
                        r.download.error = None;
                        if row.download.status == Status::Cancelled {
                            r.download.downloaded = 0;
                        }
                    })
                    .ok_or("Download not found")?;
                self.schedule();
                Ok(self.get_row(id).map(|r| self.live(r.download)).unwrap_or(d))
            }
            _ => Ok(self.live(row.download)),
        }
    }

    async fn retry(self: &Arc<Self>, id: i64) -> Result<Download, String> {
        let row = self.get_row(id)?;
        if row.download.status == Status::Completed {
            // "Download again": a new entry with the same link and settings.
            return self
                .add(AddParams {
                    url: row.download.url.clone(),
                    file_name: Some(row.download.file_name.clone()),
                    directory: Some(row.download.destination_dir.clone()),
                    headers: Some(row.headers.clone()),
                    source: Some(row.download.source),
                    page_url: row.download.page_url.clone(),
                    variant: row.variant_id.clone(),
                    title: None,
                    start: Some(true),
                })
                .await;
        }
        self.resume(id)
    }

    fn cancel(self: &Arc<Self>, id: i64) -> Result<Download, String> {
        let row = self.get_row(id)?;
        if row.download.status == Status::Completed {
            return Err("This download has already finished.".into());
        }
        let running = lock(&self.jobs).get(&id).cloned();
        match running {
            Some(job) => job.stop(StopReason::Cancel),
            None => {
                remove_temp_files(&row.download);
                let _ = self.storage.clear_segments(id);
            }
        }
        self.modify(id, |r| {
            r.download.status = Status::Cancelled;
            r.download.downloaded = 0;
            r.download.error = None;
        })
        .ok_or_else(|| "Download not found".into())
    }

    async fn remove(self: &Arc<Self>, id: i64, delete_file: bool) -> Result<(), String> {
        let row = self.get_row(id)?;
        let running = lock(&self.jobs).get(&id).cloned();
        if let Some(job) = running {
            job.stop(StopReason::Cancel);
            for _ in 0..100 {
                if !lock(&self.jobs).contains_key(&id) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        if row.download.status != Status::Completed {
            remove_temp_files(&row.download);
        } else if delete_file {
            let p = Path::new(&row.download.destination_path);
            if p.exists() {
                std::fs::remove_file(p).map_err(|e| format!("Couldn't delete the file: {e}"))?;
            }
        }
        self.storage.delete(id).map_err(|e| e.to_string())?;
        self.emit(Event::Removed { id });
        self.schedule();
        Ok(())
    }

    fn rename(self: &Arc<Self>, id: i64, new_name: &str) -> Result<Download, String> {
        let row = self.get_row(id)?;
        if row.download.status == Status::Downloading {
            return Err("Pause the download before renaming it.".into());
        }
        let name = files::keep_extension(new_name, &row.download.file_name);
        if name.is_empty() {
            return Err("Enter a file name.".into());
        }
        let dir = PathBuf::from(&row.download.destination_dir);
        let old = PathBuf::from(&row.download.destination_path);
        let new_path = files::unique_path(&dir, &name, &|p| p == old.as_path());
        if row.download.status == Status::Completed && old.exists() {
            std::fs::rename(&old, &new_path).map_err(|e| format!("Couldn't rename the file: {e}"))?;
        } else {
            let old_part = files::part_path(&old);
            if old_part.exists() {
                std::fs::rename(&old_part, files::part_path(&new_path)).map_err(|e| format!("Couldn't rename the file: {e}"))?;
            }
            for t in stream::temp_paths(&old) {
                if t.exists() {
                    let mut target = new_path.as_os_str().to_owned();
                    target.push(".yadparts");
                    let _ = std::fs::rename(&t, PathBuf::from(target));
                }
            }
        }
        let final_name = new_path.file_name().and_then(|n| n.to_str()).unwrap_or(&name).to_string();
        let path = new_path.to_string_lossy().to_string();
        self.modify(id, |r| {
            r.download.file_name = final_name.clone();
            r.download.destination_path = path.clone();
            r.download.file_type = if r.download.kind == Kind::File { FileType::for_name(&final_name) } else { r.download.file_type };
        })
        .ok_or_else(|| "Download not found".into())
    }

    fn pause_all(self: &Arc<Self>) {
        for r in self.storage.list().unwrap_or_default() {
            if r.download.status.is_active() {
                let _ = self.pause(r.download.id);
            }
        }
    }

    fn resume_all(self: &Arc<Self>) {
        for r in self.storage.list().unwrap_or_default() {
            if r.download.status == Status::Paused {
                let _ = self.modify(r.download.id, |r| r.download.status = Status::Queued);
            }
        }
        self.schedule();
    }

    fn set_settings(self: &Arc<Self>, patch: &Value) -> Result<Settings, String> {
        let old = self.settings();
        let new = old.merged(patch)?;
        if new.start_at_login != old.start_at_login {
            // Register first so the switch never shows "on" when the system said no.
            let result = if new.start_at_login { service::install(&self.launch, &self.paths) } else { service::uninstall() };
            if let Err(e) = result {
                let what = if new.start_at_login { "set up" } else { "turn off" };
                return Err(format!("Couldn't {what} start at login: {e}"));
            }
        }
        self.storage.save_settings(&new).map_err(|e| e.to_string())?;
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = new.clone();
        self.limiter.set_rate(new.speed_limit);
        self.emit(Event::Settings { settings: new.clone() });
        self.schedule();
        Ok(new)
    }

    fn hello(&self, conn: u64, params: &Value) {
        let browser = params.get("browser").and_then(Value::as_str).map(str::to_string);
        {
            let mut ext = lock(&self.extension);
            ext.1.insert(conn);
            ext.0.connected = true;
            ext.0.last_seen = Some(now_secs());
            ext.0.browser = browser.clone();
            ext.0.version = params.get("version").and_then(Value::as_str).map(str::to_string);
        }
        self.emit(Event::Extension { connected: true, browser });
    }

    fn connection_closed(&self, conn: u64) {
        let changed = {
            let mut ext = lock(&self.extension);
            if ext.1.remove(&conn) && ext.1.is_empty() {
                ext.0.connected = false;
                ext.0.last_seen = Some(now_secs());
                Some(ext.0.browser.clone())
            } else {
                None
            }
        };
        if let Some(browser) = changed {
            self.emit(Event::Extension { connected: false, browser });
        }
    }
}

/// Default name for a stream: the page title (or link) plus quality, e.g. "Talk 1080p.mp4".
fn stream_name(base: Option<&str>, label: Option<&str>, url: &str, audio: bool) -> String {
    let base = base.map(str::trim).filter(|b| !b.is_empty()).map(str::to_string).unwrap_or_else(|| {
        let n = files::name_from_url(url);
        match files::extension(&n) {
            Some(ext) => n[..n.len() - ext.len() - 1].to_string(),
            None => n,
        }
    });
    let mut stem = files::sanitize(&base);
    if let Some(ext) = files::extension(&stem) {
        if ["mp4", "m3u8", "mpd", "m4a", "ts", "mkv", "webm"].contains(&ext.to_ascii_lowercase().as_str()) {
            stem.truncate(stem.len() - ext.len() - 1);
        }
    }
    let stem: String = stem.chars().take(120).collect();
    match (label, audio) {
        (_, true) => format!("{stem}.m4a"),
        (Some(l), false) if l != "Video" => format!("{stem} {l}.mp4"),
        _ => format!("{stem}.mp4"),
    }
}

fn remove_temp_files(d: &Download) {
    let dest = Path::new(&d.destination_path);
    let _ = std::fs::remove_file(files::part_path(dest));
    for t in stream::temp_paths(dest) {
        let _ = std::fs::remove_dir_all(t);
    }
}

async fn tick_loop(inner: Arc<Inner>) {
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    let mut n: u64 = 0;
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = inner.shutdown.cancelled() => return,
        }
        n += 1;
        let jobs: Vec<Arc<Job>> = lock(&inner.jobs).values().cloned().collect();
        if jobs.is_empty() {
            continue;
        }
        let each_second = n.is_multiple_of(4);
        let mut items = Vec::with_capacity(jobs.len());
        for job in jobs {
            let downloaded = job.progress.downloaded();
            let total = job.progress.total();
            let speed = {
                let mut stats = lock(&inner.stats);
                let s = stats.entry(job.id).or_default();
                if each_second {
                    match s.last_bytes {
                        Some(last) => {
                            let delta = downloaded.saturating_sub(last);
                            s.speed = if s.history.is_empty() { delta } else { (s.speed + delta) / 2 };
                            s.history.push_back(delta);
                            if s.history.len() > HISTORY {
                                s.history.pop_front();
                            }
                        }
                        None => s.speed = 0,
                    }
                    s.last_bytes = Some(downloaded);
                }
                s.speed
            };
            if each_second {
                let rows = job.progress.rows();
                if !rows.is_empty() {
                    if let Err(e) = inner.storage.checkpoint(job.id, &rows, downloaded, total) {
                        log::error!("checkpoint {}: {e}", job.id);
                    }
                }
            }
            items.push(ProgressItem {
                id: job.id,
                downloaded,
                total_size: total,
                speed,
                eta: match total {
                    Some(t) if speed > 0 && t > downloaded => Some((t - downloaded) / speed),
                    _ => None,
                },
                connections: job.progress.connections.load(std::sync::atomic::Ordering::SeqCst),
            });
        }
        inner.emit(Event::Progress { items });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_names() {
        assert_eq!(stream_name(Some("Fearless async"), Some("1080p"), "https://x/a.m3u8", false), "Fearless async 1080p.mp4");
        assert_eq!(stream_name(None, Some("Video"), "https://x/talk/master.m3u8", false), "master.mp4");
        assert_eq!(stream_name(Some("Talk: part 1"), None, "https://x/a.m3u8", true), "Talk_ part 1.m4a");
    }
}
