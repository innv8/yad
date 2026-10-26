//! DASH manifest parsing (ISO/IEC 23009-1): SegmentTemplate (with or without
//! SegmentTimeline), SegmentList, and single-file representations. Only the first period of
//! on-demand manifests is used; live manifests are refused.

use roxmltree::{Document, Node};
use url::Url;

use super::stream::{ByteRange, MediaRef, Track};
use crate::protocol::Variant;

#[derive(Debug, Clone, PartialEq)]
pub struct Representation {
    pub id: String,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codecs: Option<String>,
    pub audio: bool,
    pub track: Track,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Manifest {
    pub duration: Option<f64>,
    pub representations: Vec<Representation>,
}

/// `PT1H2M3.5S` → 3723.5
pub fn iso_duration(s: &str) -> Option<f64> {
    let s = s.trim().strip_prefix('P')?;
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let mut total = 0.0;
    let mut num = String::new();
    for c in date.chars() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
        } else {
            let v: f64 = num.parse().ok()?;
            num.clear();
            total += v * match c {
                'D' => 86400.0,
                'W' => 604800.0,
                _ => return None, // years/months are ambiguous; not used by VOD manifests
            };
        }
    }
    for c in time.chars() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
        } else {
            let v: f64 = num.parse().ok()?;
            num.clear();
            total += v * match c {
                'H' => 3600.0,
                'M' => 60.0,
                'S' => 1.0,
                _ => return None,
            };
        }
    }
    Some(total)
}

fn child<'a>(n: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn children<'a>(n: Node<'a, 'a>, name: &'a str) -> impl Iterator<Item = Node<'a, 'a>> + 'a {
    n.children().filter(move |c| c.is_element() && c.tag_name().name() == name)
}

fn base_url(n: Node<'_, '_>, parent: &Url) -> Url {
    child(n, "BaseURL")
        .and_then(|b| b.text())
        .and_then(|t| parent.join(t.trim()).ok())
        .unwrap_or_else(|| parent.clone())
}

/// SegmentTemplate attributes inherited from AdaptationSet to Representation.
#[derive(Debug, Clone, Default)]
struct Template {
    media: Option<String>,
    init: Option<String>,
    start_number: Option<u64>,
    timescale: Option<u64>,
    duration: Option<u64>,
    timeline: Option<Vec<(Option<u64>, u64, i64)>>,
}

impl Template {
    fn read(n: Node<'_, '_>, mut base: Template) -> Template {
        if let Some(t) = child(n, "SegmentTemplate") {
            if let Some(v) = t.attribute("media") {
                base.media = Some(v.into());
            }
            if let Some(v) = t.attribute("initialization") {
                base.init = Some(v.into());
            }
            if let Some(v) = t.attribute("startNumber").and_then(|v| v.parse().ok()) {
                base.start_number = Some(v);
            }
            if let Some(v) = t.attribute("timescale").and_then(|v| v.parse().ok()) {
                base.timescale = Some(v);
            }
            if let Some(v) = t.attribute("duration").and_then(|v| v.parse().ok()) {
                base.duration = Some(v);
            }
            if let Some(tl) = child(t, "SegmentTimeline") {
                base.timeline = Some(
                    children(tl, "S")
                        .map(|s| {
                            (
                                s.attribute("t").and_then(|v| v.parse().ok()),
                                s.attribute("d").and_then(|v| v.parse().ok()).unwrap_or(0),
                                s.attribute("r").and_then(|v| v.parse().ok()).unwrap_or(0),
                            )
                        })
                        .collect(),
                );
            }
        }
        base
    }
}

/// Fills `$RepresentationID$`, `$Number%05d$`, `$Time$`, `$Bandwidth$` and `$$`.
pub fn fill(template: &str, rep_id: &str, number: u64, time: u64, bandwidth: u64) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('$') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token = &after[..end];
        let (name, fmt) = token.split_once('%').map(|(n, f)| (n, Some(f))).unwrap_or((token, None));
        let value = match name {
            "" => "$".to_string(),
            "RepresentationID" => rep_id.to_string(),
            "Number" => number.to_string(),
            "Time" => time.to_string(),
            "Bandwidth" => bandwidth.to_string(),
            _ => format!("${token}$"),
        };
        let width: usize = fmt.and_then(|f| f.trim_start_matches('0').trim_end_matches('d').parse().ok()).unwrap_or(0);
        out.push_str(&format!("{value:0>width$}"));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn parse(xml: &str, manifest_url: &Url) -> Result<Manifest, String> {
    let doc = Document::parse(xml).map_err(|_| "The video manifest couldn't be read.".to_string())?;
    let mpd = doc.root_element();
    if mpd.attribute("type") == Some("dynamic") {
        return Err("Live streams aren't supported.".into());
    }
    let duration = mpd.attribute("mediaPresentationDuration").and_then(iso_duration);
    let base = base_url(mpd, manifest_url);
    let period = child(mpd, "Period").ok_or("The video manifest has no content.")?;
    let period_duration = period.attribute("duration").and_then(iso_duration).or(duration);
    let base = base_url(period, &base);

    let mut out = Manifest { duration: period_duration, representations: vec![] };
    for set in children(period, "AdaptationSet") {
        if child(set, "ContentProtection").is_some() {
            return Err("This video is protected (DRM) and can't be downloaded.".into());
        }
        let set_mime = set.attribute("mimeType").unwrap_or("");
        let set_type = set.attribute("contentType").unwrap_or("");
        let set_base = base_url(set, &base);
        let set_template = Template::read(set, Template::default());
        let set_list = child(set, "SegmentList");
        for rep in children(set, "Representation") {
            if child(rep, "ContentProtection").is_some() {
                return Err("This video is protected (DRM) and can't be downloaded.".into());
            }
            let mime = rep.attribute("mimeType").unwrap_or(set_mime);
            let is_audio = mime.starts_with("audio") || set_type == "audio";
            let is_video = mime.starts_with("video") || set_type == "video";
            if !is_audio && !is_video {
                continue; // subtitles, images
            }
            let id = rep.attribute("id").unwrap_or("0").to_string();
            let bandwidth: Option<u64> = rep.attribute("bandwidth").and_then(|v| v.parse().ok());
            let rep_base = base_url(rep, &set_base);
            let template = Template::read(rep, set_template.clone());
            let list = child(rep, "SegmentList").or(set_list);
            let track = if template.media.is_some() {
                template_track(&template, &id, bandwidth.unwrap_or(0), &rep_base, period_duration)?
            } else if let Some(list) = list {
                list_track(list, &rep_base)
            } else {
                // One file per representation (SegmentBase or bare BaseURL).
                Track {
                    init: None,
                    segments: vec![MediaRef { url: rep_base.to_string(), range: None, key: None, sequence: 0 }],
                    container: String::new(),
                }
            };
            let mut track = track;
            track.container = if mime.contains("webm") { "webm".into() } else { "mp4".into() };
            out.representations.push(Representation {
                id,
                bandwidth,
                width: rep.attribute("width").and_then(|v| v.parse().ok()),
                height: rep.attribute("height").and_then(|v| v.parse().ok()),
                codecs: rep.attribute("codecs").or(set.attribute("codecs")).map(str::to_string),
                audio: is_audio,
                track,
            });
        }
    }
    if out.representations.is_empty() {
        return Err("The video manifest has no playable tracks.".into());
    }
    Ok(out)
}

fn template_track(t: &Template, rep_id: &str, bandwidth: u64, base: &Url, duration: Option<f64>) -> Result<Track, String> {
    let media = t.media.as_deref().unwrap_or_default();
    let join = |s: String| base.join(&s).map(|u| u.to_string()).unwrap_or(s);
    let timescale = t.timescale.unwrap_or(1).max(1);
    let start_number = t.start_number.unwrap_or(1);
    let mut segments = Vec::new();
    if let Some(timeline) = &t.timeline {
        let mut time = 0u64;
        let mut number = start_number;
        let end_time = duration.map(|d| (d * timescale as f64) as u64);
        for (i, (t0, d, r)) in timeline.iter().enumerate() {
            if let Some(t0) = t0 {
                time = *t0;
            }
            let repeats = if *r < 0 {
                // Repeat until the next S element or the end of the period.
                let until = timeline.get(i + 1).and_then(|n| n.0).or(end_time).unwrap_or(time);
                if *d == 0 { 0 } else { until.saturating_sub(time) / d }
            } else {
                *r as u64 + 1
            };
            for _ in 0..repeats {
                segments.push(MediaRef { url: join(fill(media, rep_id, number, time, bandwidth)), range: None, key: None, sequence: number });
                time += d;
                number += 1;
            }
        }
    } else {
        let seg_dur = t.duration.ok_or("The video manifest doesn't say how long segments are.")?;
        let total = duration.ok_or("The video manifest has no duration.")?;
        let count = (total * timescale as f64 / seg_dur as f64).ceil() as u64;
        for i in 0..count {
            let number = start_number + i;
            segments.push(MediaRef { url: join(fill(media, rep_id, number, i * seg_dur, bandwidth)), range: None, key: None, sequence: number });
        }
    }
    let init = t.init.as_deref().map(|i| MediaRef { url: join(fill(i, rep_id, 0, 0, bandwidth)), range: None, key: None, sequence: 0 });
    Ok(Track { init, segments, container: String::new() })
}

fn list_track(list: Node<'_, '_>, base: &Url) -> Track {
    let range = |s: Option<&str>| -> Option<ByteRange> {
        let (a, b) = s?.split_once('-')?;
        let (a, b): (u64, u64) = (a.parse().ok()?, b.parse().ok()?);
        Some(ByteRange { offset: a, length: b - a + 1 })
    };
    let init = child(list, "Initialization").map(|i| MediaRef {
        url: i.attribute("sourceURL").and_then(|u| base.join(u).ok()).unwrap_or_else(|| base.clone()).to_string(),
        range: range(i.attribute("range")),
        key: None,
        sequence: 0,
    });
    let segments = children(list, "SegmentURL")
        .enumerate()
        .map(|(i, s)| MediaRef {
            url: s.attribute("media").and_then(|u| base.join(u).ok()).unwrap_or_else(|| base.clone()).to_string(),
            range: range(s.attribute("mediaRange")),
            key: None,
            sequence: i as u64 + 1,
        })
        .collect();
    Track { init, segments, container: String::new() }
}

pub fn variants(m: &Manifest) -> Vec<Variant> {
    let size = |bw: Option<u64>| Some((bw? as f64 * m.duration? / 8.0) as u64);
    let best_audio = m.representations.iter().filter(|r| r.audio).max_by_key(|r| r.bandwidth);
    let mut video: Vec<&Representation> = m.representations.iter().filter(|r| !r.audio).collect();
    video.sort_by_key(|r| std::cmp::Reverse((r.height.unwrap_or(0), r.bandwidth.unwrap_or(0))));
    let mut out: Vec<Variant> = Vec::new();
    for r in video {
        let label = super::hls::label_for(r.height, r.bandwidth);
        if out.iter().any(|o| o.label == label) {
            continue;
        }
        let audio_bw = best_audio.and_then(|a| a.bandwidth).unwrap_or(0);
        out.push(Variant {
            id: format!("v:{}", r.id),
            label,
            resolution: r.width.zip(r.height).map(|(w, h)| format!("{w}x{h}")),
            bandwidth: r.bandwidth,
            codecs: r.codecs.clone(),
            estimated_size: size(r.bandwidth.map(|b| b + audio_bw)),
            audio_only: false,
        });
    }
    if let Some(a) = best_audio {
        out.push(Variant {
            id: format!("a:{}", a.id),
            label: "Audio".into(),
            resolution: None,
            bandwidth: a.bandwidth,
            codecs: a.codecs.clone(),
            estimated_size: size(a.bandwidth),
            audio_only: true,
        });
    }
    out
}

/// Returns (video, audio) representation indexes.
pub fn choose(m: &Manifest, wanted: Option<&str>, quality: &str, merge: bool) -> Option<(Option<usize>, Option<usize>)> {
    let best_audio = m.representations.iter().enumerate().filter(|(_, r)| r.audio).max_by_key(|(_, r)| r.bandwidth).map(|(i, _)| i);
    let by_id = |id: &str| m.representations.iter().position(|r| r.id == id);
    if let Some(w) = wanted {
        if let Some(i) = w.strip_prefix("v:").and_then(by_id) {
            return Some((Some(i), if merge { best_audio } else { None }));
        }
        if let Some(i) = w.strip_prefix("a:").and_then(by_id) {
            return Some((None, Some(i)));
        }
    }
    let list = variants(m);
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
    let id = pick.id.clone();
    choose(m, Some(&id), quality, merge)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT0H0M20.0S">
  <BaseURL>https://cdn.example.com/v/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4" codecs="avc1.640028">
      <SegmentTemplate timescale="1000" duration="4000" startNumber="1"
        initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number%05d$.m4s"/>
      <Representation id="1080" bandwidth="5000000" width="1920" height="1080"/>
      <Representation id="720" bandwidth="2500000" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4">
      <Representation id="aac" bandwidth="128000" codecs="mp4a.40.2">
        <SegmentTemplate timescale="48000" initialization="a/init.mp4" media="a/$Time$.m4s">
          <SegmentTimeline><S t="0" d="96000" r="4"/></SegmentTimeline>
        </SegmentTemplate>
      </Representation>
    </AdaptationSet>
    <AdaptationSet mimeType="text/vtt"><Representation id="subs" bandwidth="1"/></AdaptationSet>
  </Period>
</MPD>"#;

    fn url() -> Url {
        Url::parse("https://videos.example.com/talk/manifest.mpd").unwrap()
    }

    #[test]
    fn iso_durations() {
        assert_eq!(iso_duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(iso_duration("P1DT1S"), Some(86401.0));
        assert_eq!(iso_duration("PT20S"), Some(20.0));
    }

    #[test]
    fn template_fill() {
        assert_eq!(fill("seg-$Number%05d$.m4s", "x", 7, 0, 0), "seg-00007.m4s");
        assert_eq!(fill("$RepresentationID$/$Time$-$Bandwidth$$$.m4s", "r", 0, 9, 10), "r/9-10$.m4s");
    }

    #[test]
    fn parses_templates_and_timelines() {
        let m = parse(MPD, &url()).unwrap();
        assert_eq!(m.duration, Some(20.0));
        assert_eq!(m.representations.len(), 3, "subtitles skipped");
        let v = &m.representations[0];
        assert_eq!(v.track.segments.len(), 5);
        assert_eq!(v.track.segments[0].url, "https://cdn.example.com/v/1080/seg-00001.m4s");
        assert_eq!(v.track.init.as_ref().unwrap().url, "https://cdn.example.com/v/1080/init.mp4");
        let a = &m.representations[2];
        assert!(a.audio);
        assert_eq!(a.track.segments.len(), 5);
        assert_eq!(a.track.segments[4].url, "https://cdn.example.com/v/a/384000.m4s");
    }

    #[test]
    fn variants_and_choice() {
        let m = parse(MPD, &url()).unwrap();
        let v = variants(&m);
        assert_eq!(v.iter().map(|v| v.label.as_str()).collect::<Vec<_>>(), ["1080p", "720p", "Audio"]);
        assert_eq!(v[0].estimated_size, Some(12_820_000));
        assert_eq!(choose(&m, None, "best", true), Some((Some(0), Some(2))));
        assert_eq!(choose(&m, None, "720", false), Some((Some(1), None)));
        assert_eq!(choose(&m, Some("a:aac"), "best", true), Some((None, Some(2))));
    }

    #[test]
    fn refuses_live_and_drm() {
        assert!(parse(r#"<MPD type="dynamic"><Period/></MPD>"#, &url()).unwrap_err().contains("Live"));
        let drm = r#"<MPD><Period><AdaptationSet mimeType="video/mp4"><ContentProtection/><Representation id="1"/></AdaptationSet></Period></MPD>"#;
        assert!(parse(drm, &url()).unwrap_err().contains("protected"));
    }
}
