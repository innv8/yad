// Dev-only fake of window.__TAURI__ implementing docs/ipc.md with in-memory data.
// Loaded by assets/js/boot.js only when the URL contains ?mock. Never used by the real app.
//   ?mock            sample data like the mockup, live progress
//   ?mock&static     same data, frozen (UI tests)
//   ?mock&empty      no downloads
//   ?mock&ask        askBeforeDownload = true
//   ?mock&offline    daemon unreachable
const q = new URLSearchParams(location.search);
const STATIC = q.has('static');
const OFFLINE = q.has('offline');
const MB = 1024 * 1024;
const GB = 1024 * MB;
const now = () => Math.floor(Date.now() / 1000);
const T0 = now();
const todayAt = (h, m) => { const d = new Date(); d.setHours(h, m, 0, 0); return Math.floor(d.getTime() / 1000); };
const daysAgo = (n, h = 12) => { const d = new Date(); d.setDate(d.getDate() - n); d.setHours(h, 0, 0, 0); return Math.floor(d.getTime() / 1000); };

const listeners = new Map();
let clipboard = 'https://example.com/downloads/from-clipboard.zip';
function emitTauri(name, payload) {
  for (const cb of listeners.get(name) || []) setTimeout(() => cb({ event: name, id: 0, payload }), 0);
}
const emit = (event) => emitTauri('yad-event', event);

const HOME = '/Users/you';
let settings = {
  downloadDir: `${HOME}/Downloads/Yad`, sortByType: true, askBeforeDownload: q.has('ask'), keepRunning: true,
  startAtLogin: true, notifications: true, maxActive: 3, connectionsPerDownload: 8, speedLimit: null, retries: 5,
  videoQuality: 'best', mergeTracks: true, theme: 'system',
  browser: {
    videoButton: true, takeOver: true, types: ['Videos', 'Audio', 'Compressed', 'Programs'], minSize: MB,
    excludedSites: ['docs.google.com', 'mail.google.com'], sendCookies: true, contextMenu: true,
  },
};
const status = {
  version: '2.0.0', pid: 4242, active: 2, totalSpeed: 0, serviceInstalled: true, ffmpeg: '/opt/homebrew/bin/ffmpeg',
  extension: { connected: true, lastSeen: T0 - 120, browser: 'Chrome 142', version: '1.0' },
};

let nextId = 9;
function mk(id, o) {
  const dir = `${settings.downloadDir}/${o.fileType}`;
  return {
    id, url: o.url, fileName: o.fileName, fileType: o.fileType, kind: o.kind || 'file',
    destinationDir: dir, destinationPath: `${dir}/${o.fileName}`, totalSize: o.totalSize ?? null,
    downloaded: o.downloaded ?? 0, status: o.status, error: o.error ?? null, resumable: o.resumable ?? true,
    connections: o.connections ?? 0, speed: o.speed ?? 0, eta: o.eta ?? null, retries: o.retries ?? 0,
    source: o.source || 'app', pageUrl: o.pageUrl ?? null, variant: o.variant ?? null,
    createdAt: o.createdAt, finishedAt: o.finishedAt ?? null,
  };
}
const SAMPLE = q.has('empty') ? [] : [
  mk(8, { url: 'https://releases.ubuntu.com/24.04/ubuntu-24.04.1-desktop-amd64.iso', fileName: 'ubuntu-24.04.1-desktop-amd64.iso', fileType: 'Compressed', totalSize: 5.8 * GB, downloaded: 2.44 * GB, status: 'Downloading', connections: 8, speed: 7.9 * MB, eta: 420, createdAt: T0 - 300 }),
  mk(7, { url: 'https://cdn.example.com/talks/rustconf-keynote-2026-1080p.m3u8', fileName: 'rustconf-keynote-2026-1080p.mp4', fileType: 'Videos', kind: 'hls', totalSize: 844 * MB, downloaded: 600 * MB, status: 'Downloading', connections: 1, speed: 1.8 * MB, eta: 130, source: 'extension', pageUrl: 'https://videos.example.com/talks/rustconf-2026-keynote', variant: '1080p', createdAt: T0 - 400 }),
  mk(6, { url: 'https://podcasts.example.com/ep/podcast-episode-142.mp3', fileName: 'podcast-episode-142.mp3', fileType: 'Audio', totalSize: 56 * MB, downloaded: 6.7 * MB, status: 'Paused', createdAt: T0 - 500 }),
  mk(5, { url: 'https://download.gimp.org/gimp/v2.10/macos/gimp-2.10.38-arm64-1.dmg', fileName: 'gimp-2.10.38-arm64-1.dmg', fileType: 'Programs', totalSize: 288 * MB, downloaded: 104 * MB, status: 'Failed', error: 'Connection lost', retries: 3, createdAt: T0 - 600 }),
  mk(4, { url: 'https://www.stats.govt.nz/assets/productivity-statistics-1978-2023.csv', fileName: 'productivity-statistics-1978-2023.csv', fileType: 'Documents', totalSize: 2.6 * MB, downloaded: 2.6 * MB, status: 'Completed', createdAt: todayAt(0, 30), finishedAt: todayAt(0, 31) }),
  mk(3, { url: 'https://get.videolan.org/vlc/4.0.0/vlc-4.0.0-dev-win64-debug-c80f7c9c.7z', fileName: 'vlc-4.0.0-dev-win64-debug-c80f7c9c.7z', fileType: 'Compressed', totalSize: 126.7 * MB, downloaded: 126.7 * MB, status: 'Completed', createdAt: daysAgo(1), finishedAt: daysAgo(1) }),
  mk(2, { url: 'https://example.com/reports/annual-report-2025.pdf', fileName: 'annual-report-2025.pdf', fileType: 'Documents', totalSize: 8.1 * MB, downloaded: 8.1 * MB, status: 'Completed', createdAt: daysAgo(2), finishedAt: daysAgo(2) }),
  mk(1, { url: 'https://example.com/wallpaper-pack-4k.zip', fileName: 'wallpaper-pack-4k.zip', fileType: 'Compressed', totalSize: 393 * MB, downloaded: 393 * MB, status: 'Completed', createdAt: daysAgo(3), finishedAt: daysAgo(3) }),
];
for (const d of SAMPLE) d.downloaded = Math.round(d.downloaded), d.totalSize = d.totalSize && Math.round(d.totalSize);
const db = new Map(SAMPLE.map((d) => [d.id, d]));
const history = new Map();
const SEG_FILL = [78, 64, 51, 47, 40, 33, 12, 9];

const EXT = {
  Videos: /\.(mp4|mkv|webm|mov|avi|m3u8|mpd)$/i, Audio: /\.(mp3|m4a|flac|wav|ogg|aac)$/i,
  Documents: /\.(pdf|docx?|xlsx?|csv|txt|epub|pptx?)$/i, Programs: /\.(exe|msi|dmg|pkg|deb|rpm|appimage)$/i,
  Images: /\.(png|jpe?g|gif|webp|svg|heic)$/i, Compressed: /\.(zip|7z|rar|tar|gz|xz|bz2|iso|zst)$/i,
};
const typeOf = (name) => Object.keys(EXT).find((t) => EXT[t].test(name)) || 'Others';
function nameOf(url) {
  try { return decodeURIComponent(new URL(url).pathname.split('/').filter(Boolean).pop() || 'download'); } catch { return 'download'; }
}
function checkUrl(url) {
  try { const u = new URL(url); if (!/^(https?|ftp):$/.test(u.protocol)) throw 0; } catch { throw { message: 'Invalid URL' }; }
}
const need = (id) => { const d = db.get(id); if (!d) throw { message: 'No such download' }; return d; };
const copy = (x) => JSON.parse(JSON.stringify(x));
function updated(d) { emit({ type: 'updated', download: copy(d) }); return copy(d); }
function merge(a, b) {
  const out = { ...a };
  for (const [k, v] of Object.entries(b || {})) out[k] = v && typeof v === 'object' && !Array.isArray(v) && a[k] ? { ...a[k], ...v } : v;
  return out;
}

function details(d) {
  const n = d.kind === 'file' && d.resumable ? 8 : 1;
  const total = d.totalSize || d.downloaded || 1;
  const size = Math.floor(total / n);
  const ratio = d.totalSize ? d.downloaded / d.totalSize : 0;
  const segments = Array.from({ length: n }, (_, i) => {
    const start = i * size;
    const end = i === n - 1 ? total - 1 : start + size - 1;
    const f = d.status === 'Completed' ? 1 : n === 8 && d.id === 8 ? SEG_FILL[i] / 100 : ratio;
    return { start, end, downloaded: Math.round((end - start + 1) * f) };
  });
  if (!history.has(d.id)) {
    const base = d.speed || 2 * MB;
    history.set(d.id, Array.from({ length: 60 }, (_, i) => (d.status === 'Downloading' ? Math.round(base * (0.62 + 0.3 * Math.sin(i / 5) * Math.cos(i / 11) + i / 200)) : 0)));
  }
  return { ...copy(d), segments, speedHistory: history.get(d.id).slice(-60), etag: d.resumable ? '"5f2a-61c0"' : null, lastModified: null };
}

const methods = {
  ping: () => 'pong',
  status: () => ({ ...copy(status), active: [...db.values()].filter((d) => d.status === 'Downloading').length, totalSpeed: [...db.values()].reduce((s, d) => s + d.speed, 0) }),
  subscribe: () => true,
  list: () => [...db.values()].sort((a, b) => b.createdAt - a.createdAt).map(copy),
  get: ({ id }) => details(need(id)),
  probe: ({ url }) => {
    checkUrl(url);
    const fileName = nameOf(url);
    const hls = /\.m3u8$/i.test(fileName);
    return {
      url, fileName: hls ? fileName.replace(/\.m3u8$/i, '.mp4') : fileName, fileType: typeOf(fileName), size: hls ? null : 844 * MB,
      contentType: hls ? 'application/vnd.apple.mpegurl' : 'video/mp4', resumable: !hls, kind: hls ? 'hls' : 'file',
      variants: hls ? [
        { id: 'v1080', label: '1080p', resolution: '1920x1080', bandwidth: 5e6, codecs: 'avc1,mp4a', estimatedSize: 844 * MB, audioOnly: false },
        { id: 'v720', label: '720p', resolution: '1280x720', bandwidth: 2.5e6, codecs: 'avc1,mp4a', estimatedSize: 412 * MB, audioOnly: false },
        { id: 'a', label: 'Audio', resolution: null, bandwidth: 128e3, codecs: 'mp4a', estimatedSize: 39 * MB, audioOnly: true },
      ] : [],
      duration: hls ? 2470 : null,
    };
  },
  add: (p) => {
    checkUrl(p.url);
    const dup = [...db.values()].find((x) => x.url === p.url && ['Queued', 'Downloading', 'Paused'].includes(x.status));
    if (dup) return copy(dup);
    const hls = /\.m3u8$/i.test(nameOf(p.url));
    const fileName = p.fileName || (hls && p.title ? `${p.title} ${p.variant === 'a' ? 'Audio' : '1080p'}.mp4` : nameOf(p.url).replace(/\.m3u8$/i, '.mp4'));
    const fileType = typeOf(fileName);
    const d = mk(nextId++, {
      url: p.url, fileName, fileType, totalSize: 120 * MB, downloaded: 0, status: p.start === false ? 'Paused' : 'Downloading',
      connections: p.start === false ? 0 : 8, speed: p.start === false ? 0 : 3 * MB, eta: p.start === false ? null : 40, source: p.source || 'app', pageUrl: p.pageUrl || null, createdAt: now(),
    });
    if (p.directory) { d.destinationDir = p.directory; d.destinationPath = `${p.directory}/${fileName}`; }
    db.set(d.id, d);
    emit({ type: 'added', download: copy(d) });
    return copy(d);
  },
  pause: ({ id }) => { const d = need(id); Object.assign(d, { status: 'Paused', speed: 0, eta: null, connections: 0 }); return updated(d); },
  resume: ({ id }) => { const d = need(id); Object.assign(d, { status: 'Downloading', speed: 2.5 * MB, connections: d.resumable ? 8 : 1, error: null }); return updated(d); },
  retry: ({ id }) => { const d = need(id); Object.assign(d, { status: 'Downloading', speed: 2.5 * MB, connections: 8, error: null, finishedAt: null, downloaded: d.status === 'Completed' ? 0 : d.downloaded }); return updated(d); },
  cancel: ({ id }) => { const d = need(id); Object.assign(d, { status: 'Cancelled', speed: 0, eta: null, connections: 0 }); return updated(d); },
  remove: ({ id }) => { need(id); db.delete(id); emit({ type: 'removed', id }); return true; },
  deleteFile: ({ id }) => { need(id); db.delete(id); emit({ type: 'removed', id }); return true; },
  rename: ({ id, fileName }) => { const d = need(id); if (!fileName || /[\\/]/.test(fileName)) throw { message: 'Invalid file name' }; d.fileName = fileName; d.destinationPath = `${d.destinationDir}/${fileName}`; return updated(d); },
  pauseAll: () => { for (const d of db.values()) if (d.status === 'Downloading' || d.status === 'Queued') methods.pause(d); return true; },
  resumeAll: () => { for (const d of db.values()) if (d.status === 'Paused') methods.resume(d); return true; },
  getSettings: () => copy(settings),
  setSettings: (p) => { settings = merge(settings, p); if ('startAtLogin' in p) status.serviceInstalled = !!p.startAtLogin; emit({ type: 'settings', settings: copy(settings) }); return copy(settings); },
  installService: () => { status.serviceInstalled = true; return methods.status(); },
  uninstallService: () => { status.serviceInstalled = false; return methods.status(); },
  hello: () => methods.status(),
  shutdown: () => true,
};

async function invoke(cmd, args = {}) {
  await new Promise((r) => setTimeout(r, 5));
  if (cmd === 'ipc') {
    if (OFFLINE) throw 'Could not connect to the YAD service';
    const fn = methods[args.method];
    if (!fn) throw `Unknown method ${args.method}`;
    try { return fn(args.params || {}); } catch (e) { throw e && e.message ? e.message : String(e); }
  }
  if (['open_path', 'reveal_path', 'show_main', 'quit_app'].includes(cmd)) { console.info('[mock]', cmd, args); return null; }
  if (cmd.startsWith('plugin:')) return null;
  throw `Unknown command ${cmd}`;
}

const fakeWindow = {
  minimize: async () => console.info('[mock] minimize'),
  toggleMaximize: async () => console.info('[mock] toggleMaximize'),
  close: async () => console.info('[mock] close'),
  isMaximized: async () => false,
};

window.__TAURI__ = {
  core: { invoke },
  event: {
    listen: async (name, cb) => {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name).add(cb);
      return () => listeners.get(name).delete(cb);
    },
    emit: async (name, payload) => emitTauri(name, payload),
  },
  window: { getCurrentWindow: () => fakeWindow },
  dialog: { open: async () => `${HOME}/Movies` },
  clipboardManager: {
    readText: async () => clipboard,
    writeText: async (t) => { clipboard = String(t); },
  },
};
window.__YAD_MOCK__ = { db, emit, settings: () => settings };

if (q.get('anchor')) setTimeout(() => emitTauri('tray-anchor', { edge: q.get('anchor') }), 20);
if (OFFLINE) setTimeout(() => emitTauri('yad-connection', { connected: false }), 50);

if (!STATIC) {
  setInterval(() => {
    const items = [];
    for (const d of db.values()) {
      if (d.status !== 'Downloading') continue;
      const s = Math.max(0, d.speed * (0.85 + Math.random() * 0.3));
      d.speed = Math.round(s);
      d.downloaded = Math.min(d.totalSize || Infinity, d.downloaded + Math.round(s / 4));
      d.eta = d.totalSize && s ? Math.round((d.totalSize - d.downloaded) / s) : null;
      const h = history.get(d.id);
      if (h) { h.push(d.speed); if (h.length > 60) h.shift(); }
      items.push({ id: d.id, downloaded: d.downloaded, totalSize: d.totalSize, speed: d.speed, eta: d.eta, connections: d.connections });
      if (d.totalSize && d.downloaded >= d.totalSize) {
        Object.assign(d, { status: 'Completed', speed: 0, eta: null, connections: 0, finishedAt: now() });
        updated(d);
      }
    }
    if (items.length) emit({ type: 'progress', items });
  }, 250);
}
