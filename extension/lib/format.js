// Formatting helpers shared by the background worker, popup and options page.

/** "844 MB", "8.1 MB", "2.4 GB". Binary units, like YAD. */
export function formatSize(bytes) {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return '';
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v < 10 ? v.toFixed(1).replace(/\.0$/, '') : Math.round(v)} ${units[i]}`;
}

/** HTML-escape text for innerHTML templates. */
export function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

export function hostOf(url) {
  try { return new URL(url).hostname.replace(/^www\./, ''); } catch { return ''; }
}

/** Last path segment of a URL, decoded. */
export function fileNameFromUrl(url) {
  try {
    const seg = new URL(url).pathname.split('/').filter(Boolean).pop() || '';
    return decodeURIComponent(seg);
  } catch { return ''; }
}

/** File name from a Content-Disposition header value, or ''. */
export function fileNameFromDisposition(value) {
  if (!value) return '';
  const star = /filename\*\s*=\s*([^']*)'[^']*'([^;]+)/i.exec(value);
  if (star) { try { return decodeURIComponent(star[2].trim().replace(/^"|"$/g, '')); } catch { /* fall through */ } }
  const plain = /filename\s*=\s*("([^"]*)"|[^;]+)/i.exec(value);
  return plain ? (plain[2] ?? plain[1]).trim() : '';
}

/** Make a page title safe as a file name (all desktop platforms). */
export function safeFileName(title, max = 120) {
  const s = String(title || '').replace(/[\\/:*?"<>|\u0000-\u001f]+/g, ' ').replace(/\s+/g, ' ').trim().replace(/^\.+|\.+$/g, '');
  return s.slice(0, max).trim();
}

/** True when `host` is `site` or a subdomain of it. */
export function hostMatches(host, site) {
  host = (host || '').toLowerCase();
  site = (site || '').toLowerCase().trim().replace(/^\*\./, '').replace(/^www\./, '');
  if (!host || !site) return false;
  host = host.replace(/^www\./, '');
  return host === site || host.endsWith(`.${site}`);
}

export function isExcluded(url, excludedSites) {
  let host = '';
  try { host = new URL(url).hostname; } catch { return false; }
  return (excludedSites || []).some((s) => hostMatches(host, s));
}

/** Normalise a site the user typed ("https://www.x.com/a" → "x.com"). */
export function normaliseSite(input) {
  let s = String(input || '').trim().toLowerCase();
  if (!s) return '';
  if (!/^[a-z][a-z0-9+.-]*:\/\//.test(s)) s = `http://${s}`;
  try {
    const h = new URL(s).hostname.replace(/^www\./, '');
    return /^[a-z0-9.-]+\.[a-z0-9-]+$|^localhost$/.test(h) ? h : '';
  } catch { return ''; }
}

/** "Chrome 142" from the user agent. */
export function browserLabel() {
  const brands = globalThis.navigator?.userAgentData?.brands || [];
  const b = brands.find((x) => /^(Google Chrome|Chromium|Microsoft Edge|Brave|Opera)$/.test(x.brand));
  if (b) return `${b.brand.replace('Google ', '')} ${b.version}`;
  const m = /Chrome\/(\d+)/.exec(globalThis.navigator?.userAgent || '');
  return m ? `Chrome ${m[1]}` : 'Chrome';
}

/** "2.0.3" → "2.0". */
export function shortVersion(v) {
  const parts = String(v || '').split('.');
  return parts.length > 2 ? parts.slice(0, 2).join('.') : String(v || '');
}

const CODECS = [
  [/^(avc1|avc3)/, 'H.264'], [/^(hvc1|hev1)/, 'H.265'], [/^(vp09|vp9)/, 'VP9'], [/^vp8/, 'VP8'], [/^(av01)/, 'AV1'],
  [/^mp4a\.40\.(2|5|29)/, 'AAC'], [/^mp4a/, 'AAC'], [/^(ac-3)/, 'AC-3'], [/^(ec-3)/, 'E-AC-3'], [/^opus/, 'Opus'],
  [/^flac/, 'FLAC'], [/^vorbis/, 'Vorbis'], [/^mp3|^mp4a\.6b/, 'MP3'],
];

function prettyCodec(c) {
  const k = c.trim().toLowerCase();
  for (const [re, name] of CODECS) if (re.test(k)) return name;
  return '';
}

/** Pixel height of a variant ("1920x1080" or "1080p"), 0 when unknown. */
export function variantHeight(v) {
  const r = /(\d+)\s*x\s*(\d+)/i.exec(v.resolution || '');
  if (r) return Number(r[2]);
  const l = /(\d{3,4})p/i.exec(v.label || '');
  return l ? Number(l[1]) : 0;
}

/**
 * Decorate a probe Variant for display: "MP4 · H.264 + AAC", "M4A · AAC 128 kbps".
 * YAD decides the actual output container; these labels describe what it normally produces.
 */
export function describeVariant(v) {
  const names = [...new Set((v.codecs || '').split(',').map(prettyCodec).filter(Boolean))];
  if (v.audioOnly) {
    const kbps = v.bandwidth ? `${Math.round(v.bandwidth / 1000)} kbps` : '';
    return { container: 'M4A', detail: ['M4A', [names[0] || 'AAC', kbps].filter(Boolean).join(' ')].join(' · ') };
  }
  const codecs = names.length ? names.join(' + ') : (v.resolution || '');
  return { container: 'MP4', detail: ['MP4', codecs].filter(Boolean).join(' · ') };
}

/** Sort: video variants by height (desc) then bandwidth, audio-only last. */
export function sortVariants(variants) {
  return [...variants].sort((a, b) => (a.audioOnly - b.audioOnly)
    || (variantHeight(b) - variantHeight(a)) || ((b.bandwidth || 0) - (a.bandwidth || 0)));
}

/**
 * Pick the variant matching Settings.videoQuality ("best" | "1080" | "720" | "480" | "audio").
 * Falls back to the closest lower quality, then the lowest higher one.
 */
export function pickVariant(variants, quality = 'best') {
  if (!variants?.length) return null;
  const sorted = sortVariants(variants);
  const video = sorted.filter((v) => !v.audioOnly);
  if (quality === 'audio') return sorted.find((v) => v.audioOnly) || video[video.length - 1] || null;
  if (!video.length) return sorted[0];
  const want = Number(quality);
  if (!want) return video[0];
  return video.find((v) => variantHeight(v) && variantHeight(v) <= want) || video[video.length - 1];
}
