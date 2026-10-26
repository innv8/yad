//! HLS playlist parsing (RFC 8216): master playlists for qualities, media playlists for
//! segments, AES-128 keys and fMP4 init sections.

use url::Url;

use super::stream::{ByteRange, Key, MediaRef, Track};
use crate::protocol::Variant;

#[derive(Debug, Clone, PartialEq)]
pub struct HlsVariant {
    pub uri: String,
    pub bandwidth: Option<u64>,
    pub resolution: Option<(u32, u32)>,
    pub codecs: Option<String>,
    pub audio_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsAudio {
    pub group: String,
    pub name: String,
    pub language: Option<String>,
    pub uri: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Master {
    pub variants: Vec<HlsVariant>,
    pub audios: Vec<HlsAudio>,
}

pub fn is_master(text: &str) -> bool {
    text.contains("#EXT-X-STREAM-INF")
}

/// Splits `A=1,B="x,y",C=0x10` respecting quotes.
pub fn attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut key = String::new();
    let mut val = String::new();
    let mut in_key = true;
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            '=' if in_key => in_key = false,
            ',' if !quoted => {
                out.push((key.trim().to_string(), val.trim().to_string()));
                key.clear();
                val.clear();
                in_key = true;
            }
            c if in_key => key.push(c),
            c => val.push(c),
        }
    }
    if !key.trim().is_empty() {
        out.push((key.trim().to_string(), val.trim().to_string()));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn resolve(base: &Url, uri: &str) -> String {
    base.join(uri).map(|u| u.to_string()).unwrap_or_else(|_| uri.to_string())
}

pub fn parse_master(text: &str, base: &Url) -> Master {
    let mut m = Master::default();
    let mut pending: Option<Vec<(String, String)>> = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(attributes(rest));
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA:") {
            let a = attributes(rest);
            if attr(&a, "TYPE") == Some("AUDIO") {
                m.audios.push(HlsAudio {
                    group: attr(&a, "GROUP-ID").unwrap_or_default().to_string(),
                    name: attr(&a, "NAME").unwrap_or("Audio").to_string(),
                    language: attr(&a, "LANGUAGE").map(str::to_string),
                    uri: attr(&a, "URI").map(|u| resolve(base, u)),
                    default: attr(&a, "DEFAULT") == Some("YES"),
                });
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some(a) = pending.take() {
                let resolution = attr(&a, "RESOLUTION").and_then(|r| {
                    let (w, h) = r.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                });
                m.variants.push(HlsVariant {
                    uri: resolve(base, line),
                    bandwidth: attr(&a, "BANDWIDTH").and_then(|b| b.parse().ok()),
                    resolution,
                    codecs: attr(&a, "CODECS").map(str::to_string),
                    audio_group: attr(&a, "AUDIO").map(str::to_string),
                });
            }
        }
    }
    m
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Media {
    pub track: Track,
    pub duration: f64,
    pub ended: bool,
}

pub fn parse_media(text: &str, base: &Url) -> Result<Media, String> {
    let mut media = Media::default();
    let mut seq: u64 = 0;
    let mut key: Option<Key> = None;
    let mut next_range: Option<ByteRange> = None;
    let mut last_range_end: u64 = 0;
    let mut have_inf = false;
    for line in text.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("#EXTINF:") {
            media.duration += v.split(',').next().and_then(|d| d.trim().parse::<f64>().ok()).unwrap_or(0.0);
            have_inf = true;
        } else if let Some(v) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            let range = parse_byterange(v, last_range_end);
            if let Some(r) = range {
                last_range_end = r.offset + r.length;
            }
            next_range = range;
        } else if let Some(v) = line.strip_prefix("#EXT-X-KEY:") {
            let a = attributes(v);
            key = match attr(&a, "METHOD") {
                Some("NONE") | None => None,
                Some("AES-128") => Some(Key {
                    uri: resolve(base, attr(&a, "URI").ok_or("A stream key has no address.")?),
                    iv: attr(&a, "IV").and_then(parse_iv),
                }),
                Some(_) => return Err("This video is protected (DRM) and can't be downloaded.".into()),
            };
        } else if let Some(v) = line.strip_prefix("#EXT-X-MAP:") {
            let a = attributes(v);
            if let Some(uri) = attr(&a, "URI") {
                media.track.init = Some(MediaRef {
                    url: resolve(base, uri),
                    range: attr(&a, "BYTERANGE").and_then(|r| parse_byterange(r, 0)),
                    key: None,
                    sequence: 0,
                });
            }
        } else if line == "#EXT-X-ENDLIST" || line.starts_with("#EXT-X-PLAYLIST-TYPE:VOD") {
            media.ended = true;
        } else if !line.is_empty() && !line.starts_with('#') && have_inf {
            media.track.segments.push(MediaRef {
                url: resolve(base, line),
                range: next_range.take(),
                key: key.clone(),
                sequence: seq,
            });
            seq += 1;
            have_inf = false;
        }
    }
    if media.track.segments.is_empty() {
        return Err("The video playlist has no segments.".into());
    }
    let first = media.track.segments[0].url.to_ascii_lowercase();
    media.track.container = if media.track.init.is_some() || first.contains(".m4s") || first.contains(".mp4") {
        "mp4".into()
    } else if first.contains(".aac") {
        "aac".into()
    } else {
        "ts".into()
    };
    Ok(media)
}

fn parse_byterange(v: &str, default_offset: u64) -> Option<ByteRange> {
    let v = v.trim().trim_matches('"');
    let (len, off) = match v.split_once('@') {
        Some((l, o)) => (l.parse().ok()?, o.parse().ok()?),
        None => (v.parse().ok()?, default_offset),
    };
    Some(ByteRange { offset: off, length: len })
}

fn parse_iv(v: &str) -> Option<[u8; 16]> {
    let hex = v.trim().trim_start_matches("0x").trim_start_matches("0X");
    let hex = format!("{hex:0>32}");
    let mut iv = [0u8; 16];
    for (i, b) in iv.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(iv)
}

pub fn label_for(height: Option<u32>, bandwidth: Option<u64>) -> String {
    match (height, bandwidth) {
        (Some(h), _) => format!("{h}p"),
        (None, Some(b)) => format!("{} kbps", b / 1000),
        _ => "Video".into(),
    }
}

fn audio_only(v: &HlsVariant) -> bool {
    v.resolution.is_none() && v.codecs.as_deref().is_some_and(|c| c.split(',').all(|c| c.trim().starts_with("mp4a") || c.trim().starts_with("ac-3") || c.trim().starts_with("ec-3") || c.trim().starts_with("opus")))
}

/// Qualities shown to the user. Video variants are sorted best first, audio last.
pub fn variants(m: &Master, duration: Option<f64>) -> Vec<Variant> {
    let size = |bw: Option<u64>| -> Option<u64> { Some((bw? as f64 * duration? / 8.0) as u64) };
    let mut video: Vec<(usize, &HlsVariant)> = m.variants.iter().enumerate().filter(|(_, v)| !audio_only(v)).collect();
    video.sort_by_key(|(_, v)| std::cmp::Reverse((v.resolution.map(|r| r.1).unwrap_or(0), v.bandwidth.unwrap_or(0))));
    let mut out: Vec<Variant> = Vec::new();
    for (i, v) in video {
        let label = label_for(v.resolution.map(|r| r.1), v.bandwidth);
        if out.iter().any(|o| o.label == label) {
            continue; // same height at a lower bitrate
        }
        out.push(Variant {
            id: format!("v{i}"),
            label,
            resolution: v.resolution.map(|(w, h)| format!("{w}x{h}")),
            bandwidth: v.bandwidth,
            codecs: v.codecs.clone(),
            estimated_size: size(v.bandwidth),
            audio_only: false,
        });
    }
    let mut audio_added = false;
    for (i, a) in m.audios.iter().enumerate().filter(|(_, a)| a.uri.is_some()) {
        if audio_added && !a.default {
            continue;
        }
        out.push(Variant {
            id: format!("a{i}"),
            label: "Audio".into(),
            resolution: None,
            bandwidth: None,
            codecs: None,
            estimated_size: None,
            audio_only: true,
        });
        audio_added = true;
        break;
    }
    if !audio_added {
        if let Some((i, v)) = m.variants.iter().enumerate().filter(|(_, v)| audio_only(v)).max_by_key(|(_, v)| v.bandwidth) {
            out.push(Variant {
                id: format!("v{i}"),
                label: "Audio".into(),
                resolution: None,
                bandwidth: v.bandwidth,
                codecs: v.codecs.clone(),
                estimated_size: size(v.bandwidth),
                audio_only: true,
            });
        }
    }
    out
}

/// Picks the variant to download: the one asked for, else by the quality setting.
pub fn choose(m: &Master, wanted: Option<&str>, quality: &str) -> Option<(Option<usize>, Option<usize>)> {
    if let Some(id) = wanted {
        if let Some(i) = id.strip_prefix('v').and_then(|n| n.parse::<usize>().ok()).filter(|i| *i < m.variants.len()) {
            return Some((Some(i), audio_for(m, i)));
        }
        if let Some(i) = id.strip_prefix('a').and_then(|n| n.parse::<usize>().ok()).filter(|i| *i < m.audios.len()) {
            return Some((None, Some(i)));
        }
    }
    let list = variants(m, None);
    let pick = match quality {
        "audio" => list.iter().find(|v| v.audio_only),
        "best" => list.iter().find(|v| !v.audio_only),
        q => {
            let max: u32 = q.parse().unwrap_or(u32::MAX);
            list.iter()
                .filter(|v| !v.audio_only)
                .find(|v| v.label.trim_end_matches('p').parse::<u32>().is_ok_and(|h| h <= max))
                .or_else(|| list.iter().rev().find(|v| !v.audio_only))
        }
    }
    .or_else(|| list.first())?;
    choose(m, Some(&pick.id), quality)
}

/// The separate audio rendition that goes with a video variant, if any.
fn audio_for(m: &Master, variant: usize) -> Option<usize> {
    let group = m.variants[variant].audio_group.as_deref()?;
    let in_group: Vec<(usize, &HlsAudio)> =
        m.audios.iter().enumerate().filter(|(_, a)| a.group == group && a.uri.is_some()).collect();
    in_group.iter().find(|(_, a)| a.default).or(in_group.first()).map(|(i, _)| *i)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="English",LANGUAGE="en",DEFAULT=YES,URI="audio/en.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS="avc1.4d401e,mp4a.40.2",AUDIO="aud"
360/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,CODECS="avc1.640028,mp4a.40.2",AUDIO="aud"
https://cdn.example.com/1080/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720,CODECS="avc1.64001f,mp4a.40.2",AUDIO="aud"
720/index.m3u8
"#;

    fn base() -> Url {
        Url::parse("https://videos.example.com/talks/master.m3u8").unwrap()
    }

    #[test]
    fn attribute_lists_keep_quoted_commas() {
        let a = attributes(r#"BANDWIDTH=1,CODECS="avc1,mp4a",RESOLUTION=1x2"#);
        assert_eq!(attr(&a, "CODECS"), Some("avc1,mp4a"));
        assert_eq!(attr(&a, "RESOLUTION"), Some("1x2"));
    }

    #[test]
    fn master_variants_sorted_with_audio() {
        let m = parse_master(MASTER, &base());
        assert!(is_master(MASTER));
        assert_eq!(m.variants.len(), 3);
        assert_eq!(m.variants[0].uri, "https://videos.example.com/talks/360/index.m3u8");
        let v = variants(&m, Some(100.0));
        let labels: Vec<_> = v.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(labels, ["1080p", "720p", "360p", "Audio"]);
        assert_eq!(v[0].estimated_size, Some(62_500_000));
        assert!(v[3].audio_only);
    }

    #[test]
    fn choose_by_quality_and_id() {
        let m = parse_master(MASTER, &base());
        assert_eq!(choose(&m, None, "best"), Some((Some(1), Some(0))));
        assert_eq!(choose(&m, None, "720"), Some((Some(2), Some(0))));
        assert_eq!(choose(&m, None, "480"), Some((Some(0), Some(0))));
        assert_eq!(choose(&m, None, "audio"), Some((None, Some(0))));
        assert_eq!(choose(&m, Some("v0"), "best"), Some((Some(0), Some(0))));
    }

    #[test]
    fn media_playlist_with_key_map_and_ranges() {
        let text = r#"#EXTM3U
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:7
#EXT-X-MAP:URI="init.mp4"
#EXT-X-KEY:METHOD=AES-128,URI="https://keys.example.com/k1",IV=0x0000000000000000000000000000000A
#EXTINF:6.0,
seg7.m4s
#EXT-X-KEY:METHOD=NONE
#EXTINF:4.5,
#EXT-X-BYTERANGE:1000@0
all.m4s
#EXTINF:4.5,
#EXT-X-BYTERANGE:500
all.m4s
#EXT-X-ENDLIST
"#;
        let m = parse_media(text, &base()).unwrap();
        assert!(m.ended);
        assert!((m.duration - 15.0).abs() < 1e-9);
        let t = &m.track;
        assert_eq!(t.container, "mp4");
        assert_eq!(t.init.as_ref().unwrap().url, "https://videos.example.com/talks/init.mp4");
        assert_eq!(t.segments.len(), 3);
        assert_eq!(t.segments[0].sequence, 7);
        assert_eq!(t.segments[0].key.as_ref().unwrap().iv.unwrap()[15], 10);
        assert!(t.segments[1].key.is_none());
        assert_eq!(t.segments[1].range, Some(ByteRange { offset: 0, length: 1000 }));
        assert_eq!(t.segments[2].range, Some(ByteRange { offset: 1000, length: 500 }));
    }

    #[test]
    fn sample_aes_is_refused() {
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXTINF:1,\na.ts\n";
        assert!(parse_media(text, &base()).unwrap_err().contains("protected"));
    }
}
