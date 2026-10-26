//! Segmented download of a single file over HTTP ranges.
//!
//! The file is written to `<name>.yadpart` with positional writes, so segments never wait
//! on each other. Each segment's position is checkpointed by the manager; a resumed
//! download continues from those positions. When a segment finishes early, the largest
//! remaining one is split in two so slow tails don't hold up completion.

use futures_util::StreamExt;
use reqwest::{header, StatusCode};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
};
use tokio::task::JoinSet;

use super::{finalize, write_all_at, Ctx, EngineError, EngineResult, Job, Outcome, SegState, PER_HOST_LIMIT};
use crate::{
    files::{self, part_path},
    http::{self, describe_error, describe_status, is_permanent, parse_content_range},
    storage::DownloadRow,
    util::backoff,
};

const MIB: u64 = 1024 * 1024;
/// Files smaller than this use one connection.
const MIN_SPLIT_SIZE: u64 = 4 * MIB;
/// Don't split a segment with less than this left.
const MIN_STEAL: u64 = 2 * MIB;

enum SegmentEnd {
    Done,
    Stopped,
    /// The server ignored the range or the file changed: start over without ranges.
    Restart,
}

pub async fn run(ctx: &Ctx, mut row: DownloadRow, job: Arc<Job>) -> EngineResult<Outcome> {
    let id = row.download.id;
    let probe = tokio::select! {
        p = http::probe(&ctx.client, &row.download.url, &row.headers) => p.map_err(EngineError::Message)?,
        _ = job.cancel.cancelled() => return Ok(Outcome::Stopped),
    };
    if probe.kind != crate::protocol::Kind::File {
        // The link turned out to be a playlist: hand over to the stream engine.
        ctx.host.update(id, &mut |r| {
            r.download.kind = probe.kind;
            r.download.file_type = crate::files::FileType::Videos;
        });
        return Ok(Outcome::Restream);
    }

    let mut segments = ctx.storage.segments(id).map_err(|e| e.to_string())?;
    let started = segments.iter().any(|s| s.downloaded > 0) || row.download.downloaded > 0;
    let changed = started
        && (probe.size != row.download.total_size
            || (row.etag.is_some() && probe.etag.is_some() && probe.etag != row.etag)
            || (row.last_modified.is_some() && probe.last_modified.is_some() && probe.last_modified != row.last_modified));
    let mut part = part_path(Path::new(&row.download.destination_path));
    if changed || !probe.resumable || (started && !part.exists()) {
        // Can't continue from where we stopped: the file changed, the server has no ranges,
        // or the partial file is gone.
        segments.clear();
        let _ = std::fs::remove_file(&part);
    }

    // First start: take the server's file name (Content-Disposition) unless the user chose one.
    if segments.is_empty() && !started && row.download.file_name == files::name_from_url(&row.download.url) && probe.file_name != row.download.file_name {
        let dir = PathBuf::from(&row.download.destination_dir);
        let dest = files::unique_path(&dir, &probe.file_name, &|_| false);
        row.download.file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or(&probe.file_name).to_string();
        row.download.destination_path = dest.to_string_lossy().to_string();
        part = part_path(&dest);
    }

    ctx.host.update(id, &mut |r| {
        r.download.total_size = probe.size;
        r.download.resumable = probe.resumable;
        r.etag = probe.etag.clone();
        r.last_modified = probe.last_modified.clone();
        r.download.file_name = row.download.file_name.clone();
        r.download.destination_path = row.download.destination_path.clone();
        if r.download.file_type == crate::files::FileType::Others {
            r.download.file_type = probe.file_type;
        }
    });
    row.download.total_size = probe.size;
    row.download.resumable = probe.resumable;
    row.etag = probe.etag.clone();

    std::fs::create_dir_all(&row.download.destination_dir)?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&part)?;
    if segments.is_empty() {
        file.set_len(0)?;
    }
    if let Some(total) = probe.size {
        if probe.resumable && file.metadata()?.len() < total {
            file.set_len(total)?;
        }
    }
    let file = Arc::new(file);

    let connections = ctx
        .settings
        .connections_per_download
        .min(PER_HOST_LIMIT.saturating_sub(ctx.host_connections_in_use))
        .max(1);

    let segs: Vec<Arc<SegState>> = if segments.is_empty() {
        plan(probe.size, probe.resumable, connections)
    } else {
        segments.iter().map(|s| SegState::new(s.idx, s.start, s.end, s.downloaded, s.done)).collect()
    };
    job.progress.total.store(probe.size.unwrap_or(0), Ordering::SeqCst);
    job.progress.set_segments(segs.clone());
    job.progress.stream_count.store(0, Ordering::SeqCst);

    let can_split = probe.resumable && probe.size.is_some();
    let worker = Worker {
        client: ctx.client.clone(),
        url: probe.final_url.clone(),
        headers: row.headers.clone(),
        if_range: row.etag.clone().filter(|e| !e.starts_with("W/")).or(row.last_modified.clone()),
        resumable: probe.resumable,
        file: file.clone(),
        job: job.clone(),
        limiter: ctx.limiter.clone(),
        retries: ctx.settings.retries,
        host: ctx.host.clone(),
        id,
    };

    let mut set = JoinSet::new();
    let mut next_idx = segs.iter().map(|s| s.idx).max().unwrap_or(0) + 1;
    for seg in segs.iter().filter(|s| !s.done.load(Ordering::SeqCst)) {
        let w = worker.clone();
        let seg = seg.clone();
        set.spawn(async move { w.run(seg).await });
    }
    job.progress.connections.store(set.len() as u32, Ordering::SeqCst);

    let mut failure: Option<EngineError> = None;
    let mut restart = false;
    while let Some(res) = set.join_next().await {
        let res = res.unwrap_or_else(|e| Err(EngineError::Message(format!("Internal error: {e}"))));
        match res {
            Ok(SegmentEnd::Done) => {
                if can_split && failure.is_none() && !job.cancel.is_cancelled() {
                    if let Some(new_seg) = steal(&job, next_idx) {
                        next_idx += 1;
                        let w = worker.clone();
                        set.spawn(async move { w.run(new_seg).await });
                    }
                }
            }
            Ok(SegmentEnd::Stopped) => {}
            Ok(SegmentEnd::Restart) => {
                restart = true;
                job.cancel.cancel();
            }
            Err(e) => {
                if failure.is_none() {
                    failure = Some(e);
                }
                // One segment gave up after its retries: stop the rest and report.
                job.cancel.cancel();
            }
        }
        job.progress.connections.store(set.len() as u32, Ordering::SeqCst);
    }
    job.progress.connections.store(0, Ordering::SeqCst);

    if restart && job.reason() == super::StopReason::None {
        ctx.storage.clear_segments(id).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&part);
        return Err("The file changed on the server while downloading. Retry to start again.".into());
    }
    if let Some(e) = failure {
        if job.reason() == super::StopReason::None {
            return Err(e);
        }
    }
    if job.cancel.is_cancelled() {
        return Ok(Outcome::Stopped);
    }

    let downloaded = job.progress.downloaded();
    if let Some(total) = probe.size {
        if downloaded < total {
            return Err(format!("Download ended early ({downloaded} of {total} bytes).").into());
        }
    }
    file.sync_all()?;
    drop(file);
    let final_path = finalize(&part, Path::new(&row.download.destination_path))?;
    let final_name = final_path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let final_str = final_path.to_string_lossy().to_string();
    ctx.host.update(id, &mut |r| {
        r.download.destination_path = final_str.clone();
        r.download.file_name = final_name.clone();
        r.download.total_size = Some(downloaded);
        r.download.downloaded = downloaded;
    });
    Ok(Outcome::Completed)
}

/// Splits a file into up to `connections` segments of at least 1 MiB.
pub fn plan(size: Option<u64>, resumable: bool, connections: u32) -> Vec<Arc<SegState>> {
    match size {
        Some(total) if resumable && total >= MIN_SPLIT_SIZE => {
            let n = (connections as u64).min(total / MIB).max(1);
            let chunk = total / n;
            (0..n)
                .map(|i| {
                    let start = i * chunk;
                    let end = if i == n - 1 { total - 1 } else { start + chunk - 1 };
                    SegState::new(i as i64, start, end, 0, false)
                })
                .collect()
        }
        Some(total) if total > 0 => vec![SegState::new(0, 0, total - 1, 0, false)],
        _ => vec![SegState::new(0, 0, u64::MAX, 0, false)],
    }
}

/// Takes the second half of the segment with the most bytes left.
pub fn steal(job: &Job, idx: i64) -> Option<Arc<SegState>> {
    let segs = job.progress.snapshot();
    let victim = segs
        .iter()
        .filter(|s| !s.done.load(Ordering::SeqCst))
        .filter_map(|s| {
            let end = s.end.load(Ordering::SeqCst);
            let pos = s.start + s.downloaded.load(Ordering::SeqCst);
            (end != u64::MAX && end >= pos).then(|| (s.clone(), end + 1 - pos, pos, end))
        })
        .max_by_key(|(_, left, _, _)| *left)?;
    let (seg, left, pos, end) = victim;
    if left < MIN_STEAL {
        return None;
    }
    let mid = pos + left / 2;
    // Only shrink if the victim hasn't moved past the split point meanwhile.
    if seg.end.compare_exchange(end, mid - 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return None;
    }
    let new_seg = SegState::new(idx, mid, end, 0, false);
    job.progress.push(new_seg.clone());
    Some(new_seg)
}

#[derive(Clone)]
struct Worker {
    client: reqwest::Client,
    url: String,
    headers: crate::protocol::Headers,
    if_range: Option<String>,
    resumable: bool,
    file: Arc<File>,
    job: Arc<Job>,
    limiter: Arc<crate::util::RateLimiter>,
    retries: u32,
    host: Arc<dyn super::Host>,
    id: i64,
}

impl Worker {
    async fn run(&self, seg: Arc<SegState>) -> EngineResult<SegmentEnd> {
        let mut failures = 0u32;
        loop {
            if self.job.cancel.is_cancelled() {
                return Ok(SegmentEnd::Stopped);
            }
            let before = seg.downloaded.load(Ordering::SeqCst);
            match self.attempt(&seg).await {
                Ok(end) => return Ok(end),
                Err(Attempt::Permanent(msg)) => return Err(msg.into()),
                Err(Attempt::Transient(msg)) => {
                    if seg.downloaded.load(Ordering::SeqCst) > before {
                        failures = 0; // made progress: the connection is just flaky
                    }
                    failures += 1;
                    if failures > self.retries {
                        return Err(msg.into());
                    }
                    self.host.note_retry(self.id);
                    if !self.resumable {
                        // No ranges: the only way to retry is from the beginning.
                        seg.downloaded.store(0, Ordering::SeqCst);
                        self.file.set_len(0)?;
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(backoff(failures)) => {}
                        _ = self.job.cancel.cancelled() => return Ok(SegmentEnd::Stopped),
                    }
                }
            }
        }
    }

    async fn attempt(&self, seg: &SegState) -> Result<SegmentEnd, Attempt> {
        let mut pos = seg.start + seg.downloaded.load(Ordering::SeqCst);
        let end = seg.end.load(Ordering::SeqCst);
        if end != u64::MAX && pos > end {
            seg.done.store(true, Ordering::SeqCst);
            return Ok(SegmentEnd::Done);
        }
        let mut req = http::with_headers(self.client.get(&self.url), &self.headers);
        if self.resumable {
            let range = if end == u64::MAX { format!("bytes={pos}-") } else { format!("bytes={pos}-{end}") };
            req = req.header(header::RANGE, range);
            if let Some(v) = &self.if_range {
                req = req.header(header::IF_RANGE, v);
            }
        }
        let resp = tokio::select! {
            r = req.send() => r.map_err(|e| Attempt::Transient(describe_error(&e)))?,
            _ = self.job.cancel.cancelled() => return Ok(SegmentEnd::Stopped),
        };
        let status = resp.status();
        if !status.is_success() {
            let msg = describe_status(status);
            return Err(if is_permanent(status) { Attempt::Permanent(msg) } else { Attempt::Transient(msg) });
        }
        if self.resumable {
            if status != StatusCode::PARTIAL_CONTENT {
                // 200 to a ranged request: ranges unsupported now, or If-Range said the file changed.
                return Ok(SegmentEnd::Restart);
            }
            let ok = resp
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range)
                .is_some_and(|(s, _, _)| s == pos);
            if !ok {
                return Ok(SegmentEnd::Restart);
            }
        }

        let mut stream = resp.bytes_stream();
        loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = self.job.cancel.cancelled() => return Ok(SegmentEnd::Stopped),
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| Attempt::Transient(describe_error(&e)))?;
            // The segment may have been split while we were reading.
            let end = seg.end.load(Ordering::SeqCst);
            let room = if end == u64::MAX { chunk.len() as u64 } else { (end + 1).saturating_sub(pos) };
            let take = (chunk.len() as u64).min(room) as usize;
            if take > 0 {
                self.limiter.acquire(take).await;
                write_all_at(&self.file, &chunk[..take], pos).map_err(|e| Attempt::Permanent(format!("Couldn't write the file: {e}")))?;
                pos += take as u64;
                seg.downloaded.fetch_add(take as u64, Ordering::SeqCst);
            }
            if end != u64::MAX && pos > end {
                seg.done.store(true, Ordering::SeqCst);
                return Ok(SegmentEnd::Done);
            }
        }
        let end = seg.end.load(Ordering::SeqCst);
        if end == u64::MAX || pos > end {
            seg.done.store(true, Ordering::SeqCst);
            if end == u64::MAX {
                // Unknown length: the end of the body is the end of the file.
                seg.end.store(pos.saturating_sub(1), Ordering::SeqCst);
            }
            Ok(SegmentEnd::Done)
        } else {
            Err(Attempt::Transient("Connection lost".into()))
        }
    }
}

enum Attempt {
    Transient(String),
    Permanent(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_splits_large_files_only() {
        let segs = plan(Some(100 * MIB), true, 8);
        assert_eq!(segs.len(), 8);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[7].end.load(Ordering::SeqCst), 100 * MIB - 1);
        let covered: u64 = segs.iter().map(|s| s.length().unwrap()).sum();
        assert_eq!(covered, 100 * MIB);
        assert_eq!(plan(Some(3 * MIB), true, 8).len(), 1);
        assert_eq!(plan(Some(100 * MIB), false, 8).len(), 1);
        assert_eq!(plan(Some(5 * MIB), true, 8).len(), 5, "at least 1 MiB per segment");
        assert_eq!(plan(None, true, 8)[0].end.load(Ordering::SeqCst), u64::MAX);
    }

    #[test]
    fn steal_halves_the_biggest_remaining_segment() {
        let job = Job::new(1);
        let a = SegState::new(0, 0, 10 * MIB - 1, 10 * MIB, true);
        let b = SegState::new(1, 10 * MIB, 30 * MIB - 1, 4 * MIB, false);
        job.progress.set_segments(vec![a, b.clone()]);
        let new = steal(&job, 2).unwrap();
        let pos = 14 * MIB;
        let mid = pos + (30 * MIB - pos) / 2;
        assert_eq!(b.end.load(Ordering::SeqCst), mid - 1);
        assert_eq!(new.start, mid);
        assert_eq!(new.end.load(Ordering::SeqCst), 30 * MIB - 1);
        assert_eq!(job.progress.snapshot().len(), 3);
    }

    #[test]
    fn steal_skips_small_remainders() {
        let job = Job::new(1);
        job.progress.set_segments(vec![SegState::new(0, 0, 3 * MIB, 2 * MIB, false)]);
        assert!(steal(&job, 1).is_none());
    }
}
