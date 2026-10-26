//! Streamed video (HLS and DASH). Media segments are the unit of work: they download in
//! parallel into `<name>.yadparts/`, each finished segment is a resume point, and the
//! tracks are joined at the end (with ffmpeg when available).

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use futures_util::StreamExt;
use reqwest::header;
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
};
use tokio::task::JoinSet;
use url::Url;

use super::{dash, finalize, hls, Ctx, EngineError, EngineResult, Job, Outcome, SegState, PER_HOST_LIMIT};
use crate::{
    http::{self, describe_error, describe_status, is_permanent},
    protocol::{Headers, Kind, Variant},
    util::{backoff, find_ffmpeg},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub uri: String,
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaRef {
    pub url: String,
    pub range: Option<ByteRange>,
    pub key: Option<Key>,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    pub init: Option<MediaRef>,
    pub segments: Vec<MediaRef>,
    /// "ts", "mp4", "aac" or "webm".
    pub container: String,
}

const IDX_STRIDE: i64 = 1_000_000;

async fn fetch_text(client: &reqwest::Client, url: &str, headers: &Headers) -> Result<(String, Url), String> {
    let resp = http::with_headers(client.get(url), headers).send().await.map_err(|e| describe_error(&e))?;
    if !resp.status().is_success() {
        return Err(describe_status(resp.status()));
    }
    let final_url = resp.url().clone();
    let text = resp.text().await.map_err(|e| describe_error(&e))?;
    Ok((text, final_url))
}

pub struct StreamInfo {
    pub variants: Vec<Variant>,
    pub duration: Option<f64>,
}

/// Lists the qualities in a stream. HLS needs one media playlist for the duration.
pub async fn probe(client: &reqwest::Client, url: &str, headers: &Headers, kind: Kind) -> Result<StreamInfo, String> {
    let (text, base) = fetch_text(client, url, headers).await?;
    match kind {
        Kind::Hls if hls::is_master(&text) => {
            let master = hls::parse_master(&text, &base);
            let first = master.variants.iter().max_by_key(|v| v.bandwidth).ok_or("The video playlist is empty.")?;
            let (media_text, media_base) = fetch_text(client, &first.uri, headers).await?;
            let media = hls::parse_media(&media_text, &media_base)?;
            if !media.ended {
                return Err("Live streams aren't supported.".into());
            }
            Ok(StreamInfo { variants: hls::variants(&master, Some(media.duration)), duration: Some(media.duration) })
        }
        Kind::Hls => {
            let media = hls::parse_media(&text, &base)?;
            if !media.ended {
                return Err("Live streams aren't supported.".into());
            }
            Ok(StreamInfo {
                variants: vec![Variant {
                    id: "v0".into(),
                    label: "Video".into(),
                    resolution: None,
                    bandwidth: None,
                    codecs: None,
                    estimated_size: None,
                    audio_only: false,
                }],
                duration: Some(media.duration),
            })
        }
        Kind::Dash => {
            let m = dash::parse(&text, &base)?;
            Ok(StreamInfo { variants: dash::variants(&m), duration: m.duration })
        }
        Kind::File => Err("Not a stream".into()),
    }
}

/// Resolves the tracks to download: video and/or audio for the chosen variant.
pub async fn tracks(
    client: &reqwest::Client,
    url: &str,
    headers: &Headers,
    kind: Kind,
    variant: Option<&str>,
    quality: &str,
    merge: bool,
) -> Result<Vec<Track>, String> {
    let (text, base) = fetch_text(client, url, headers).await?;
    let mut uris: Vec<String> = Vec::new();
    match kind {
        Kind::Hls if hls::is_master(&text) => {
            let master = hls::parse_master(&text, &base);
            let (v, a) = hls::choose(&master, variant, quality).ok_or("The video playlist is empty.")?;
            if let Some(v) = v {
                uris.push(master.variants[v].uri.clone());
            }
            if let Some(a) = a {
                if merge || v.is_none() {
                    if let Some(u) = &master.audios[a].uri {
                        uris.push(u.clone());
                    }
                }
            }
        }
        Kind::Hls => {
            let media = hls::parse_media(&text, &base)?;
            return Ok(vec![media.track]);
        }
        Kind::Dash => {
            let m = dash::parse(&text, &base)?;
            let (v, a) = dash::choose(&m, variant, quality, merge).ok_or("The video manifest has no tracks.")?;
            return Ok([v, a].into_iter().flatten().map(|i| m.representations[i].track.clone()).collect());
        }
        Kind::File => return Err("Not a stream".into()),
    }
    let mut out = Vec::new();
    for u in uris {
        let (t, b) = fetch_text(client, &u, headers).await?;
        let media = hls::parse_media(&t, &b)?;
        if !media.ended {
            return Err("Live streams aren't supported.".into());
        }
        out.push(media.track);
    }
    Ok(out)
}

fn parts_dir(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".yadparts");
    PathBuf::from(s)
}

pub fn temp_paths(dest: &Path) -> Vec<PathBuf> {
    vec![parts_dir(dest)]
}

struct Shared {
    client: reqwest::Client,
    headers: Headers,
    dir: PathBuf,
    job: Arc<Job>,
    limiter: Arc<crate::util::RateLimiter>,
    retries: u32,
    host: Arc<dyn super::Host>,
    id: i64,
    keys: Mutex<HashMap<String, [u8; 16]>>,
    queue: Mutex<VecDeque<(Arc<SegState>, MediaRef)>>,
}

pub async fn run(ctx: &Ctx, row: crate::storage::DownloadRow, job: Arc<Job>) -> EngineResult<Outcome> {
    let id = row.download.id;
    let tracks = tokio::select! {
        t = tracks(&ctx.client, &row.download.url, &row.headers, row.download.kind, row.variant_id.as_deref(),
                   &ctx.settings.video_quality, ctx.settings.merge_tracks) => t.map_err(EngineError::Message)?,
        _ = job.cancel.cancelled() => return Ok(Outcome::Stopped),
    };
    let dest = PathBuf::from(&row.download.destination_path);
    let dir = parts_dir(&dest);
    std::fs::create_dir_all(&dir)?;

    let saved: HashMap<i64, crate::storage::SegmentRow> =
        ctx.storage.segments(id).map_err(|e| e.to_string())?.into_iter().map(|s| (s.idx, s)).collect();
    let mut all = Vec::new();
    let mut work = VecDeque::new();
    let mut count = 0u64;
    for (t, track) in tracks.iter().enumerate() {
        let refs = track.init.iter().map(|r| (0i64, r)).chain(track.segments.iter().enumerate().map(|(i, r)| (i as i64 + 1, r)));
        for (pos, r) in refs {
            let idx = t as i64 * IDX_STRIDE + pos;
            let file = dir.join(format!("{idx}.seg"));
            let done_before = saved.get(&idx).filter(|s| s.done && file.exists());
            let seg = SegState::new(idx, 0, u64::MAX, done_before.map_or(0, |s| s.downloaded), done_before.is_some());
            if done_before.is_none() {
                work.push_back((seg.clone(), r.clone()));
            }
            all.push(seg);
            count += 1;
        }
    }
    job.progress.set_segments(all);
    job.progress.stream_count.store(count, Ordering::SeqCst);
    job.progress.total.store(row.download.total_size.unwrap_or(0), Ordering::SeqCst);

    let shared = Arc::new(Shared {
        client: ctx.client.clone(),
        headers: row.headers.clone(),
        dir: dir.clone(),
        job: job.clone(),
        limiter: ctx.limiter.clone(),
        retries: ctx.settings.retries,
        host: ctx.host.clone(),
        id,
        keys: Mutex::new(HashMap::new()),
        queue: Mutex::new(work),
    });
    let workers = ctx
        .settings
        .connections_per_download
        .min(PER_HOST_LIMIT.saturating_sub(ctx.host_connections_in_use))
        .max(1);
    let mut set = JoinSet::new();
    for _ in 0..workers {
        let s = shared.clone();
        set.spawn(async move { worker(s).await });
    }
    job.progress.connections.store(workers, Ordering::SeqCst);
    let mut failure = None;
    while let Some(res) = set.join_next().await {
        if let Err(e) = res.unwrap_or_else(|e| Err(EngineError::Message(format!("Internal error: {e}")))) {
            if failure.is_none() {
                failure = Some(e);
            }
            job.cancel.cancel();
        }
        job.progress.connections.store(set.len() as u32, Ordering::SeqCst);
    }
    if let Some(e) = failure {
        if job.reason() == super::StopReason::None {
            return Err(e);
        }
    }
    if job.cancel.is_cancelled() {
        return Ok(Outcome::Stopped);
    }

    // Join each track's pieces in order.
    let mut track_files = Vec::new();
    for (t, track) in tracks.iter().enumerate() {
        let ext = match track.container.as_str() {
            "ts" => "ts",
            "aac" => "aac",
            "webm" => "webm",
            _ => "mp4",
        };
        let out_path = dir.join(format!("track{t}.{ext}"));
        let mut out = std::fs::File::create(&out_path)?;
        let n = track.segments.len() as i64 + 1;
        for pos in 0..n {
            let p = dir.join(format!("{}.seg", t as i64 * IDX_STRIDE + pos));
            if pos == 0 && !p.exists() {
                continue; // no init section
            }
            let mut f = std::fs::File::open(&p)?;
            std::io::copy(&mut f, &mut out)?;
        }
        out.sync_all()?;
        track_files.push((out_path, ext));
    }

    let audio_only = tracks.len() == 1 && row.download.variant.as_deref() == Some("Audio");
    let (final_path, note) = assemble(&track_files, &dest, audio_only).await?;
    let _ = std::fs::remove_dir_all(&dir);
    let size = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
    let name = final_path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let path_str = final_path.to_string_lossy().to_string();
    ctx.host.update(id, &mut |r| {
        r.download.destination_path = path_str.clone();
        r.download.file_name = name.clone();
        r.download.total_size = Some(size);
        r.download.downloaded = size;
        r.download.error = note.clone();
    });
    Ok(Outcome::Completed)
}

async fn assemble(tracks: &[(PathBuf, &str)], dest: &Path, audio_only: bool) -> EngineResult<(PathBuf, Option<String>)> {
    let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("video").to_string();
    let dir = dest.parent().unwrap_or(Path::new(".")).to_path_buf();
    let webm = tracks.iter().any(|(_, e)| *e == "webm");
    let out_ext = if audio_only { "m4a" } else if webm { "mkv" } else { "mp4" };
    let wanted = dir.join(format!("{stem}.{out_ext}"));

    if let Some(ffmpeg) = find_ffmpeg() {
        let tmp = tracks[0].0.with_file_name(format!("joined.{out_ext}"));
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        for (p, _) in tracks {
            cmd.arg("-i").arg(p);
        }
        if tracks.len() == 2 {
            cmd.args(["-map", "0:v:0?", "-map", "1:a:0?"]);
        }
        cmd.args(["-c", "copy"]);
        if out_ext != "mkv" {
            cmd.args(["-movflags", "+faststart"]);
        }
        cmd.arg(&tmp);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        match cmd.output().await {
            Ok(out) if out.status.success() && tmp.exists() => {
                return Ok((finalize(&tmp, &wanted)?, None));
            }
            Ok(out) => log::warn!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr)),
            Err(e) => log::warn!("couldn't run ffmpeg: {e}"),
        }
    }

    // Without ffmpeg: keep the tracks as they are.
    let (video, vext) = &tracks[0];
    let first = finalize(video, &dir.join(format!("{stem}.{}", if *vext == "aac" { "aac" } else { vext })))?;
    if tracks.len() == 2 {
        let (audio, aext) = &tracks[1];
        let ext = if *aext == "mp4" { "m4a" } else { aext };
        finalize(audio, &dir.join(format!("{stem}.audio.{ext}")))?;
        return Ok((first, Some("Video and audio were saved as separate files. Install ffmpeg to have YAD join them.".into())));
    }
    Ok((first, None))
}

async fn worker(s: Arc<Shared>) -> EngineResult<()> {
    loop {
        let next = s.queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
        let Some((seg, r)) = next else { return Ok(()) };
        let mut failures = 0;
        loop {
            if s.job.cancel.is_cancelled() {
                return Ok(());
            }
            match fetch_segment(&s, &seg, &r).await {
                Ok(()) => break,
                Err(Fetch::Stopped) => return Ok(()),
                Err(Fetch::Permanent(m)) => return Err(m.into()),
                Err(Fetch::Transient(m)) => {
                    failures += 1;
                    if failures > s.retries {
                        return Err(m.into());
                    }
                    s.host.note_retry(s.id);
                    tokio::select! {
                        _ = tokio::time::sleep(backoff(failures)) => {}
                        _ = s.job.cancel.cancelled() => return Ok(()),
                    }
                }
            }
        }
    }
}

enum Fetch {
    Stopped,
    Transient(String),
    Permanent(String),
}

async fn fetch_segment(s: &Shared, seg: &SegState, r: &MediaRef) -> Result<(), Fetch> {
    seg.downloaded.store(0, Ordering::SeqCst);
    let mut req = http::with_headers(s.client.get(&r.url), &s.headers);
    if let Some(range) = r.range {
        req = req.header(header::RANGE, format!("bytes={}-{}", range.offset, range.offset + range.length - 1));
    }
    let resp = tokio::select! {
        r = req.send() => r.map_err(|e| Fetch::Transient(describe_error(&e)))?,
        _ = s.job.cancel.cancelled() => return Err(Fetch::Stopped),
    };
    let status = resp.status();
    if !status.is_success() {
        let m = describe_status(status);
        return Err(if is_permanent(status) { Fetch::Permanent(m) } else { Fetch::Transient(m) });
    }
    let mut body = Vec::with_capacity(resp.content_length().unwrap_or(0) as usize);
    let mut stream = resp.bytes_stream();
    loop {
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = s.job.cancel.cancelled() => return Err(Fetch::Stopped),
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|e| Fetch::Transient(describe_error(&e)))?;
        s.limiter.acquire(chunk.len()).await;
        seg.downloaded.fetch_add(chunk.len() as u64, Ordering::SeqCst);
        body.extend_from_slice(&chunk);
    }
    if let Some(range) = r.range {
        if status != reqwest::StatusCode::PARTIAL_CONTENT && body.len() as u64 > range.length {
            // Server ignored the range: cut our piece out of the full body.
            let start = range.offset as usize;
            let end = (range.offset + range.length) as usize;
            body = body.get(start..end).map(|b| b.to_vec()).ok_or_else(|| Fetch::Permanent("A video segment is shorter than expected.".into()))?;
        }
    }
    if let Some(key) = &r.key {
        let k = key_bytes(s, &key.uri).await?;
        let iv = key.iv.unwrap_or_else(|| {
            let mut iv = [0u8; 16];
            iv[8..].copy_from_slice(&r.sequence.to_be_bytes());
            iv
        });
        body = cbc::Decryptor::<aes::Aes128>::new(&k.into(), &iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(&body)
            .map_err(|_| Fetch::Permanent("A video segment couldn't be decrypted.".into()))?;
    }
    let path = s.dir.join(format!("{}.seg", seg.idx));
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &body).map_err(|e| Fetch::Permanent(format!("Couldn't write the file: {e}")))?;
    std::fs::rename(&tmp, &path).map_err(|e| Fetch::Permanent(format!("Couldn't write the file: {e}")))?;
    seg.downloaded.store(body.len() as u64, Ordering::SeqCst);
    seg.done.store(true, Ordering::SeqCst);
    Ok(())
}

async fn key_bytes(s: &Shared, uri: &str) -> Result<[u8; 16], Fetch> {
    if let Some(k) = s.keys.lock().unwrap_or_else(|e| e.into_inner()).get(uri) {
        return Ok(*k);
    }
    let resp = http::with_headers(s.client.get(uri), &s.headers)
        .send()
        .await
        .map_err(|e| Fetch::Transient(describe_error(&e)))?;
    if !resp.status().is_success() {
        return Err(Fetch::Permanent(format!("Couldn't get the stream key: {}", describe_status(resp.status()))));
    }
    let bytes = resp.bytes().await.map_err(|e| Fetch::Transient(describe_error(&e)))?;
    let key: [u8; 16] = bytes.as_ref().try_into().map_err(|_| Fetch::Permanent("The stream key is invalid.".into()))?;
    s.keys.lock().unwrap_or_else(|e| e.into_inner()).insert(uri.to_string(), key);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncryptMut;

    #[test]
    fn aes_128_round_trip_matches_hls_iv_rule() {
        let key = [7u8; 16];
        let mut iv = [0u8; 16];
        iv[8..].copy_from_slice(&42u64.to_be_bytes());
        let plain = b"segment payload that is not block aligned";
        let enc = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain);
        let dec = cbc::Decryptor::<aes::Aes128>::new(&key.into(), &iv.into()).decrypt_padded_vec_mut::<Pkcs7>(&enc).unwrap();
        assert_eq!(dec, plain);
    }
}
