//! Download engine. A running download is a [`Job`]; its tasks report progress through
//! shared atomics that the manager samples for speed, checkpoints and events.

pub mod dash;
pub mod file;
pub mod hls;
pub mod stream;

use std::{
    fs::File,
    io,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
};
use tokio_util::sync::CancellationToken;

use crate::{
    protocol::Settings,
    storage::{DownloadRow, SegmentRow, Storage},
    util::RateLimiter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StopReason {
    None = 0,
    Pause = 1,
    Cancel = 2,
    Shutdown = 3,
}

pub struct Job {
    pub id: i64,
    pub cancel: CancellationToken,
    reason: AtomicU8,
    pub progress: Arc<Progress>,
}

impl Job {
    pub fn new(id: i64) -> Arc<Job> {
        Arc::new(Job {
            id,
            cancel: CancellationToken::new(),
            reason: AtomicU8::new(StopReason::None as u8),
            progress: Arc::new(Progress::default()),
        })
    }

    pub fn stop(&self, reason: StopReason) {
        // The first reason wins: a Cancel after Shutdown must not delete files.
        let _ = self.reason.compare_exchange(StopReason::None as u8, reason as u8, Ordering::SeqCst, Ordering::SeqCst);
        self.cancel.cancel();
    }

    pub fn reason(&self) -> StopReason {
        match self.reason.load(Ordering::SeqCst) {
            1 => StopReason::Pause,
            2 => StopReason::Cancel,
            3 => StopReason::Shutdown,
            _ => StopReason::None,
        }
    }
}

/// One byte range of a file (or one media segment of a stream).
pub struct SegState {
    pub idx: i64,
    pub start: u64,
    /// Inclusive. Can shrink while running when the segment is split. `u64::MAX` = unknown.
    pub end: AtomicU64,
    pub downloaded: AtomicU64,
    pub done: AtomicBool,
}

impl SegState {
    pub fn new(idx: i64, start: u64, end: u64, downloaded: u64, done: bool) -> Arc<SegState> {
        Arc::new(SegState {
            idx,
            start,
            end: AtomicU64::new(end),
            downloaded: AtomicU64::new(downloaded),
            done: AtomicBool::new(done),
        })
    }

    pub fn length(&self) -> Option<u64> {
        let end = self.end.load(Ordering::SeqCst);
        (end != u64::MAX).then(|| end + 1 - self.start)
    }

    /// Bytes counted toward progress; overlap after a split is not double counted.
    pub fn counted(&self) -> u64 {
        let d = self.downloaded.load(Ordering::SeqCst);
        self.length().map_or(d, |l| d.min(l))
    }

    pub fn row(&self) -> SegmentRow {
        SegmentRow {
            idx: self.idx,
            start: self.start,
            end: self.end.load(Ordering::SeqCst),
            downloaded: self.downloaded.load(Ordering::SeqCst),
            done: self.done.load(Ordering::SeqCst),
        }
    }
}

#[derive(Default)]
pub struct Progress {
    pub segments: Mutex<Vec<Arc<SegState>>>,
    /// 0 = unknown.
    pub total: AtomicU64,
    pub connections: AtomicU32,
    /// Streams: number of media segments, to estimate the total from the average size.
    pub stream_count: AtomicU64,
}

impl Progress {
    pub fn set_segments(&self, segs: Vec<Arc<SegState>>) {
        *self.segments.lock().unwrap_or_else(|e| e.into_inner()) = segs;
    }

    pub fn push(&self, seg: Arc<SegState>) {
        self.segments.lock().unwrap_or_else(|e| e.into_inner()).push(seg);
    }

    pub fn snapshot(&self) -> Vec<Arc<SegState>> {
        self.segments.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn downloaded(&self) -> u64 {
        self.snapshot().iter().map(|s| s.counted()).sum()
    }

    pub fn total(&self) -> Option<u64> {
        let count = self.stream_count.load(Ordering::SeqCst);
        if count > 0 {
            // Streams: average finished segment size × number of segments.
            let segs = self.snapshot();
            let done: Vec<_> = segs.iter().filter(|s| s.done.load(Ordering::SeqCst)).collect();
            if !done.is_empty() {
                let bytes: u64 = done.iter().map(|s| s.downloaded.load(Ordering::SeqCst)).sum();
                let estimate = bytes / done.len() as u64 * count;
                return Some(estimate.max(self.downloaded()));
            }
        }
        match self.total.load(Ordering::SeqCst) {
            0 => None,
            t => Some(t),
        }
    }

    pub fn rows(&self) -> Vec<SegmentRow> {
        self.snapshot().iter().map(|s| s.row()).collect()
    }
}

/// What the engine needs from the manager.
pub trait Host: Send + Sync {
    /// Changes the stored row (name, size, validators…) and tells the UI.
    fn update(&self, id: i64, f: &mut dyn FnMut(&mut DownloadRow));
    fn note_retry(&self, id: i64);
}

pub struct Ctx {
    pub client: reqwest::Client,
    pub storage: Arc<Storage>,
    pub limiter: Arc<RateLimiter>,
    pub settings: Settings,
    pub host: Arc<dyn Host>,
    /// Connections other downloads already use on this host.
    pub host_connections_in_use: u32,
}

/// Total connections YAD opens to one host across all downloads.
pub const PER_HOST_LIMIT: u32 = 16;

#[derive(Debug)]
pub enum Outcome {
    Completed,
    Stopped,
    /// A "file" link turned out to be an HLS/DASH playlist.
    Restream,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Message(String),
    #[error("Couldn't write the file: {0}")]
    Io(#[from] io::Error),
}

impl From<String> for EngineError {
    fn from(s: String) -> Self {
        EngineError::Message(s)
    }
}

impl From<&str> for EngineError {
    fn from(s: &str) -> Self {
        EngineError::Message(s.to_string())
    }
}

pub type EngineResult<T> = Result<T, EngineError>;

#[cfg(unix)]
pub fn write_all_at(f: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.write_all_at(buf, offset)
}

#[cfg(windows)]
pub fn write_all_at(f: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_write(buf, offset)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0"));
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

/// Moves the finished file into place without overwriting anything that appeared meanwhile.
pub fn finalize(part: &std::path::Path, wanted: &std::path::Path) -> io::Result<std::path::PathBuf> {
    let dest = if wanted.exists() {
        let dir = wanted.parent().unwrap_or(std::path::Path::new("."));
        let name = wanted.file_name().and_then(|n| n.to_str()).unwrap_or("download");
        crate::files::unique_path(dir, name, &|_| false)
    } else {
        wanted.to_path_buf()
    };
    std::fs::rename(part, &dest)?;
    Ok(dest)
}
