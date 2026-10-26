// YAD Link background service worker (MV3, ES module).
//
// - Watches responses (webRequest) for media files, HLS/DASH manifests and attachments, per tab.
// - Asks YAD to probe manifests for their qualities (`probe`), keeps a badge count.
// - Sends downloads to YAD (`add`) with Cookie / Referer / User-Agent headers.
// - Takes over Chrome downloads, adds the "Download with YAD" context menu.
// - Owns the single native messaging port; popup/options go through runtime messages.

import { NativeClient, NativeError } from './lib/native.js';
import { extOf, typeOf, MEDIA_EXTS, SEGMENT_EXTS } from './lib/filetypes.js';
import {
  fileNameFromUrl, fileNameFromDisposition, isExcluded, browserLabel,
  describeVariant, sortVariants, pickVariant, variantHeight,
} from './lib/format.js';

const BADGE_COLOR = '#4c6a92';
const MIN_MEDIA_BYTES = 100 * 1024;
const ALT_BYPASS_MS = 3_000;
const MENU_ID = 'yad-download';

export const DEFAULT_SETTINGS = {
  videoQuality: 'best',
  browser: {
    videoButton: true,
    takeOver: true,
    types: ['Videos', 'Audio', 'Compressed', 'Programs'],
    minSize: 1024 * 1024,
    excludedSites: [],
    sendCookies: true,
    contextMenu: true,
  },
};

// ---------------------------------------------------------------- settings cache

let settings = structuredClone(DEFAULT_SETTINGS);

async function loadSettings() {
  const { settings: s } = await chrome.storage.local.get('settings');
  if (s) settings = mergeSettings(DEFAULT_SETTINGS, s);
  return settings;
}

function mergeSettings(base, s) {
  return { ...base, ...s, browser: { ...base.browser, ...(s?.browser || {}) } };
}

async function storeSettings(full) {
  settings = mergeSettings(DEFAULT_SETTINGS, { videoQuality: full.videoQuality, browser: full.browser });
  await chrome.storage.local.set({ settings });
  await updateContextMenu();
  broadcast({ type: 'settings', settings });
  return settings;
}

/** Pull settings from YAD, first pushing any change made while YAD was offline. */
async function syncSettings() {
  const { pendingBrowser } = await chrome.storage.local.get('pendingBrowser');
  let full;
  if (pendingBrowser && Object.keys(pendingBrowser).length) {
    full = await native.call('setSettings', { browser: pendingBrowser });
    await chrome.storage.local.remove('pendingBrowser');
  } else {
    full = await native.call('getSettings');
  }
  return storeSettings(full);
}

async function setBrowserSettings(patch) {
  // Optimistic local update so the content scripts react at once.
  settings = mergeSettings(settings, { browser: patch });
  await chrome.storage.local.set({ settings });
  await updateContextMenu();
  try {
    const full = await native.call('setSettings', { browser: patch });
    return { settings: await storeSettings(full), synced: true };
  } catch (err) {
    if (!err.offline) throw err;
    const { pendingBrowser = {} } = await chrome.storage.local.get('pendingBrowser');
    await chrome.storage.local.set({ pendingBrowser: { ...pendingBrowser, ...patch } });
    broadcast({ type: 'settings', settings });
    return { settings, synced: false };
  }
}

// ---------------------------------------------------------------- native port

const native = new NativeClient({
  hello: () => ({ browser: browserLabel(), version: chrome.runtime.getManifest().version }),
  onState: (state) => {
    broadcast({ type: 'state', state: publicState() });
    if (state.connected && !wasConnected) onConnected();
    wasConnected = state.connected;
  },
});
let wasConnected = false;

function publicState() {
  const { connected, connecting, status, error } = native.state;
  return { connected, connecting, status, error };
}

let settingsSynced = Promise.resolve();
async function onConnected() {
  settingsSynced = syncSettings().catch((e) => console.warn('YAD: settings sync failed', e));
  await settingsSynced;
  // Streams found while YAD was offline only have locally parsed qualities: ask YAD again.
  await ready;
  for (const [tabId, tab] of tabs) {
    for (const item of tab.items.values()) if (item.kind !== 'file' && (!item.probed || item.localOnly)) probeItem(tabId, item);
  }
}

/** Connect if possible, quietly. Resolves true when YAD answered. */
async function reachable(timeoutMs) {
  if (native.connected) return true;
  try {
    await (timeoutMs ? Promise.race([native.connect(), new Promise((_, r) => setTimeout(() => r(new Error('timeout')), timeoutMs))]) : native.connect());
    return true;
  } catch { return false; }
}

/** Message popup/options pages; ignores "no receiver" errors. */
function broadcast(msg) {
  chrome.runtime.sendMessage(msg).catch(() => {});
}

// ---------------------------------------------------------------- per-tab media

/**
 * tabs: Map<tabId, { pageUrl, title, items: Map<url, Item> }>
 * Item: { url, kind: 'file'|'hls'|'dash'|'attachment', contentType, size, fileName, frameUrl,
 *         variants: Variant[], probed, localOnly, duration, seen }
 * Persisted in storage.session so a restarted worker keeps what it found.
 */
const tabs = new Map();
const ready = (async () => {
  await loadSettings();
  try {
    const { tabs: saved } = await chrome.storage.session.get('tabs');
    for (const [id, t] of Object.entries(saved || {})) {
      tabs.set(Number(id), { pageUrl: t.pageUrl, items: new Map(t.items.map((i) => [i.url, i])) });
    }
  } catch (e) { console.warn('YAD: could not restore tab state', e); }
})();

let saveTimer = null;
function persist() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    const out = {};
    for (const [id, t] of tabs) out[id] = { pageUrl: t.pageUrl, items: [...t.items.values()] };
    chrome.storage.session.set({ tabs: out }).catch(() => {});
  }, 300);
}

function tabState(tabId, create = true) {
  let t = tabs.get(tabId);
  if (!t && create) { t = { pageUrl: '', items: new Map() }; tabs.set(tabId, t); }
  return t;
}

const stripHash = (u) => String(u || '').split('#')[0];

function resetTab(tabId, pageUrl) {
  const t = tabs.get(tabId);
  const had = t && t.items.size;
  tabs.set(tabId, { pageUrl: stripHash(pageUrl), items: new Map() });
  if (had) { refreshTab(tabId); }
  persist();
}

/** Media visible to the user for a tab, best first, decorated for display. */
function mediaView(tabId) {
  const t = tabs.get(tabId);
  if (!t) return [];
  const items = [...t.items.values()].filter((i) => i.kind !== 'attachment');
  // A media playlist next to a master playlist is one of the master's renditions. YAD's probe
  // returns no variant or a single one for a media playlist, so a master is one with 2 or more.
  const isMaster = (i) => (i.variants?.length || 0) >= 2;
  const hasMaster = { hls: items.some((i) => i.kind === 'hls' && isMaster(i)), dash: items.some((i) => i.kind === 'dash' && isMaster(i)) };
  const quality = settings.videoQuality || 'best';
  return items
    .filter((i) => !(i.kind !== 'file' && hasMaster[i.kind] && !isMaster(i)))
    .map((i) => {
      const variants = sortVariants(i.variants || []).map((v) => ({
        id: v.id, label: v.label, audioOnly: !!v.audioOnly, size: v.estimatedSize ?? null,
        height: variantHeight(v), ...describeVariant(v),
      }));
      const preferred = pickVariant(i.variants || [], quality);
      const ext = extOf(i.fileName || i.url);
      return {
        url: i.url, kind: i.kind, size: i.size ?? null, fileName: i.fileName || fileNameFromUrl(i.url),
        label: i.kind === 'file' ? (ext ? ext.toUpperCase() : 'Video') : i.kind.toUpperCase(),
        container: (ext || (i.contentType || '').split('/')[1] || '').toUpperCase(),
        audio: /^audio\//.test(i.contentType || '') || typeOf(ext) === 'Audio',
        probing: i.kind !== 'file' && !i.probed,
        error: i.error || null,
        variants, preferred: preferred ? preferred.id : null,
        seen: i.seen,
      };
    })
    .sort((a, b) => (b.variants.length - a.variants.length) || ((b.kind !== 'file') - (a.kind !== 'file')) || ((b.size || 0) - (a.size || 0)) || (b.seen - a.seen));
}

function attachmentsView(tabId) {
  const t = tabs.get(tabId);
  if (!t) return [];
  return [...t.items.values()].filter((i) => i.kind === 'attachment')
    .map((i) => ({ url: i.url, fileName: i.fileName || fileNameFromUrl(i.url), size: i.size ?? null, contentType: i.contentType }));
}

const refreshTimers = new Map();
/** Update badge and tell the tab's content scripts (debounced per tab). */
function refreshTab(tabId) {
  clearTimeout(refreshTimers.get(tabId));
  refreshTimers.set(tabId, setTimeout(() => {
    refreshTimers.delete(tabId);
    const media = mediaView(tabId);
    chrome.action.setBadgeText({ tabId, text: media.length ? String(media.length) : '' }).catch(() => {});
    chrome.action.setBadgeBackgroundColor({ tabId, color: BADGE_COLOR }).catch(() => {});
    chrome.action.setBadgeTextColor?.({ tabId, color: '#f3f4f5' })?.catch?.(() => {});
    chrome.tabs.sendMessage(tabId, { type: 'media', items: media }).catch(() => {});
    broadcast({ type: 'tab', tabId });
  }, 150));
}

// ---------------------------------------------------------------- response classification

function header(headers, name) {
  const h = headers?.find((x) => x.name.toLowerCase() === name);
  return h ? (h.value ?? '') : '';
}

function totalSize(headers) {
  const range = /\/(\d+)\s*$/.exec(header(headers, 'content-range'));
  if (range) return Number(range[1]);
  const len = header(headers, 'content-length');
  return len && /^\d+$/.test(len) ? Number(len) : null;
}

/** Recently seen downloadable responses, for take-over (url → info). */
const recent = new Map();
function remember(url, info) {
  recent.set(url, info);
  if (recent.size > 200) recent.delete(recent.keys().next().value);
}

/** What a response is, or null when it is not interesting. */
export function classify(url, type, statusCode, headers) {
  const ct = header(headers, 'content-type').toLowerCase().split(';')[0].trim();
  const disposition = header(headers, 'content-disposition');
  const ext = extOf(url);
  const size = totalSize(headers);

  if (/^attachment/i.test(disposition.trim()) && statusCode >= 200 && statusCode < 300) {
    return { kind: 'attachment', contentType: ct, size, fileName: fileNameFromDisposition(disposition) || fileNameFromUrl(url) };
  }
  if (statusCode < 200 || statusCode >= 300) return null;
  if (ext === 'm3u8' || /^(application\/(vnd\.apple\.mpegurl|x-mpegurl)|audio\/(x-)?mpegurl)$/.test(ct)) {
    if (/^text\/html/.test(ct)) return null;
    return { kind: 'hls', contentType: ct, size: null };
  }
  if (ext === 'mpd' || ct === 'application/dash+xml') return { kind: 'dash', contentType: ct, size: null };

  if (type === 'main_frame' || type === 'sub_frame') return null;
  // Segments and fragments of streams.
  if (SEGMENT_EXTS.has(ext) || /^(video\/(mp2t|iso\.segment|vnd\.dlna\.mpeg-tts)|audio\/(mp2t|aac))$/.test(ct)) return null;
  if (/[?&](range|bytestart|byterange)=\d+/i.test(url) || /\/range\/\d+-\d+/.test(url)) return null;
  const isMedia = /^(video|audio)\//.test(ct) || (ct === 'application/octet-stream' && MEDIA_EXTS.has(ext))
    || ((!ct || ct === 'binary/octet-stream') && MEDIA_EXTS.has(ext) && type === 'media');
  if (!isMedia) return null;
  if (size != null && size < MIN_MEDIA_BYTES) return null;
  return { kind: 'file', contentType: ct, size, fileName: fileNameFromUrl(url) };
}

chrome.webRequest.onHeadersReceived.addListener((d) => {
  if (d.tabId < 0 || !/^https?:/.test(d.url)) return;
  const info = classify(d.url, d.type, d.statusCode, d.responseHeaders);
  if (d.type === 'main_frame' && info?.kind !== 'attachment' && d.statusCode >= 200 && d.statusCode < 300) {
    ready.then(() => { const t = tabs.get(d.tabId); if (!t || t.pageUrl !== stripHash(d.url)) resetTab(d.tabId, d.url); });
  }
  if (info && (info.kind === 'attachment' || d.type === 'other' || d.type === 'main_frame' || d.type === 'sub_frame')) {
    remember(stripHash(d.url), { ...info, at: Date.now() });
  }
  if (!info) return;
  ready.then(() => record(d.tabId, d.url, info, d.frameId === 0 ? null : d.initiator));
}, {
  urls: ['<all_urls>'],
  types: ['media', 'xmlhttprequest', 'other', 'main_frame', 'sub_frame'],
}, ['responseHeaders']);

function record(tabId, rawUrl, info, frameUrl) {
  const url = stripHash(rawUrl);
  const t = tabState(tabId);
  const existing = t.items.get(url);
  if (existing) {
    // Byte-range requests for the same file: keep the best size we learn.
    if (info.size && (!existing.size || info.size > existing.size) && existing.kind === 'file') { existing.size = info.size; persist(); refreshTab(tabId); }
    return;
  }
  const item = { url, ...info, frameUrl: frameUrl || null, variants: [], probed: info.kind === 'file' || info.kind === 'attachment', localOnly: false, duration: null, seen: Date.now() };
  t.items.set(url, item);
  persist();
  refreshTab(tabId);
  if (item.kind === 'hls' || item.kind === 'dash') probeItem(tabId, item);
}

// ---------------------------------------------------------------- probing

const probing = new Set();
async function probeItem(tabId, item) {
  const key = `${tabId} ${item.url}`;
  if (probing.has(key)) return;
  probing.add(key);
  try {
    let res = null;
    let probeError = null;
    if (await reachable(item.localOnly ? 0 : 3_000)) {
      try { res = await native.call('probe', { url: item.url, headers: await headersFor(item.url, tabId) }, { timeout: 20_000 }); } catch (e) {
        // e.g. "Live streams aren't supported." — shown to the user as is.
        if (!e.offline) probeError = e.message;
      }
    }
    const t = tabs.get(tabId);
    if (!t || t.items.get(item.url) !== item) return; // navigated away meanwhile
    item.error = probeError;
    if (probeError) {
      item.probed = true;
      item.localOnly = false;
    } else if (res) {
      Object.assign(item, {
        variants: res.variants || [], size: res.size ?? item.size, fileName: res.fileName || item.fileName,
        duration: res.duration ?? null, probed: true, localOnly: false,
      });
    } else if (item.kind === 'hls' && !item.variants.length) {
      item.variants = await parseMasterLocally(item.url);
      item.localOnly = true;
      item.probed = true;
    } else {
      item.probed = true;
    }
    persist();
    refreshTab(tabId);
  } finally {
    probing.delete(key);
  }
}

/**
 * Minimal HLS master playlist reader, used only while YAD is offline so the menu can still list
 * qualities. YAD does the real parsing; variant ids from here are never sent to `add`.
 */
async function parseMasterLocally(url) {
  try {
    const res = await fetch(url, { credentials: 'include' });
    if (!res.ok) return [];
    const text = await res.text();
    if (!text.startsWith('#EXTM3U') || !text.includes('#EXT-X-STREAM-INF')) return [];
    const attrs = (line) => Object.fromEntries([...line.matchAll(/([A-Z0-9-]+)=("[^"]*"|[^,]*)/g)].map((m) => [m[1], m[2].replace(/^"|"$/g, '')]));
    const out = [];
    const lines = text.split(/\r?\n/);
    lines.forEach((line, i) => {
      if (!line.startsWith('#EXT-X-STREAM-INF:')) return;
      const a = attrs(line.slice(18));
      const h = /x(\d+)/.exec(a.RESOLUTION || '');
      out.push({ id: `local:${i}`, label: h ? `${h[1]}p` : `${Math.round((+a.BANDWIDTH || 0) / 1000)} kbps`, resolution: a.RESOLUTION || null,
        bandwidth: +a.BANDWIDTH || null, codecs: a.CODECS || null, estimatedSize: null, audioOnly: false });
    });
    if (lines.some((l) => l.startsWith('#EXT-X-MEDIA:') && /TYPE=AUDIO/.test(l))) {
      out.push({ id: 'local:audio', label: 'Audio', resolution: null, bandwidth: null, codecs: null, estimatedSize: null, audioOnly: true });
    }
    // Same resolution at several bitrates: keep the highest.
    const seen = new Map();
    for (const v of out) { const k = v.audioOnly ? 'a' : v.label; if (!seen.has(k) || (v.bandwidth || 0) > (seen.get(k).bandwidth || 0)) seen.set(k, v); }
    return [...seen.values()];
  } catch { return []; }
}

// ---------------------------------------------------------------- sending to YAD

async function headersFor(url, tabId, referrer) {
  const headers = { 'User-Agent': navigator.userAgent };
  let ref = referrer;
  if (!ref && tabId != null && tabId >= 0) { try { ref = (await chrome.tabs.get(tabId)).url; } catch { /* tab gone */ } }
  if (ref && /^https?:/.test(ref)) headers.Referer = ref;
  if (settings.browser.sendCookies) {
    try {
      const cookies = await chrome.cookies.getAll({ url });
      if (cookies.length) headers.Cookie = cookies.map((c) => `${c.name}=${c.value}`).join('; ');
    } catch { /* no cookies for this URL */ }
  }
  return headers;
}

/** Send one download to YAD. Throws NativeError (err.offline when YAD is not reachable). */
async function addToYad({ url, variant, pageUrl, title, tabId, referrer, fileName }) {
  await ready;
  const t = tabId != null ? tabs.get(tabId) : null;
  const item = t?.items.get(stripHash(url));
  const params = { url, source: 'extension', headers: await headersFor(url, tabId, referrer || pageUrl) };
  if (pageUrl) params.pageUrl = pageUrl;
  if (variant && !String(variant).startsWith('local:')) params.variant = variant;
  if (variant && String(variant).startsWith('local:')) {
    // Locally parsed qualities: ask YAD for the real ones first.
    const probe = await native.call('probe', { url, headers: params.headers }, { timeout: 20_000 });
    const local = item?.variants.find((v) => v.id === variant);
    const match = local && (local.audioOnly ? probe.variants.find((v) => v.audioOnly)
      : probe.variants.find((v) => v.label === local.label) || pickVariant(probe.variants, String(variantHeight(local) || 'best')));
    if (match) params.variant = match.id;
  }
  if (fileName) params.fileName = fileName;
  // YAD names streamed videos "<title> <quality>.mp4" from the page title.
  const isStream = item ? (item.kind === 'hls' || item.kind === 'dash') : /^(m3u8|mpd)$/.test(extOf(url));
  if (title && isStream) params.title = String(title).slice(0, 300);
  return native.call('add', params);
}

async function addFromMessage(msg, sender) {
  try {
    const tabId = msg.tabId ?? sender.tab?.id;
    const download = await addToYad({ ...msg, tabId, pageUrl: msg.pageUrl || sender.tab?.url, title: msg.title || sender.tab?.title });
    return { ok: true, download };
  } catch (err) {
    return { ok: false, error: err.message, offline: !!err.offline };
  }
}

// ---------------------------------------------------------------- take over downloads

let altUntil = 0;
const letChromeHave = new Set(); // URLs we re-handed to Chrome after a failed add

chrome.downloads.onCreated.addListener((item) => { takeOver(item).catch((e) => console.warn('YAD: take-over failed', e)); });

async function takeOver(item) {
  await ready;
  const b = settings.browser;
  const url = item.finalUrl || item.url;
  if (!b.takeOver) return;
  if (item.byExtensionId === chrome.runtime.id || letChromeHave.delete(url) || letChromeHave.delete(item.url)) return;
  if (!/^https?:/.test(url)) return;            // blob:, data:, file: stay in Chrome
  if (item.state && item.state !== 'in_progress') return;
  if (Date.now() < altUntil) { altUntil = 0; return; } // Alt-click: let Chrome download it
  if (isExcluded(url, b.excludedSites) || (item.referrer && isExcluded(item.referrer, b.excludedSites))) return;

  const seen = recent.get(stripHash(url)) || recent.get(stripHash(item.url)) || {};
  const name = item.filename ? item.filename.split(/[\\/]/).pop() : (seen.fileName || fileNameFromUrl(url));
  const type = typeOf(extOf(name) || extOf(url), item.mime || seen.contentType);
  if (!b.types.includes(type)) return;
  const size = item.totalBytes > 0 ? item.totalBytes : item.fileSize > 0 ? item.fileSize : seen.size;
  if (size != null && size > 0 && size < b.minSize) return;
  if (!(await reachable(2_000))) return;        // never cancel when YAD can't take it

  try { await chrome.downloads.cancel(item.id); } catch { return; }
  chrome.downloads.erase({ id: item.id }).catch(() => {});
  try {
    const tabId = await tabIdForReferrer(item.referrer);
    await addToYad({ url, tabId, referrer: item.referrer, pageUrl: item.referrer || undefined, fileName: seen.fileName || undefined });
  } catch (err) {
    console.warn('YAD: add failed, giving the download back to Chrome', err);
    letChromeHave.add(url);
    chrome.downloads.download({ url }).catch(() => {});
  }
}

async function tabIdForReferrer(referrer) {
  if (!referrer) return null;
  try {
    const [tab] = await chrome.tabs.query({ url: stripHash(referrer) });
    return tab?.id ?? null;
  } catch { return null; }
}

// ---------------------------------------------------------------- context menu

let menuUpdate = Promise.resolve();
function updateContextMenu() {
  menuUpdate = menuUpdate.then(async () => {
    await chrome.contextMenus.removeAll();
    if (settings.browser.contextMenu) {
      chrome.contextMenus.create({ id: MENU_ID, title: 'Download with YAD', contexts: ['link', 'video', 'audio', 'image'] }, () => void chrome.runtime.lastError);
    }
  }).catch(() => {});
  return menuUpdate;
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== MENU_ID) return;
  await ready;
  let url = info.linkUrl || info.srcUrl;
  let variant;
  const tabId = tab?.id;
  if (url && !/^https?:/.test(url) && (info.mediaType === 'video' || info.mediaType === 'audio')) {
    // MSE player (blob: src): use the stream we saw for this tab.
    const best = mediaView(tabId)[0];
    if (!best) return;
    url = best.url;
    variant = best.preferred || undefined;
  }
  if (!url || !/^https?:/.test(url)) return;
  try {
    await addToYad({ url, variant, tabId, pageUrl: info.frameUrl || info.pageUrl, title: tab?.title });
  } catch (err) {
    if (err.offline) { letChromeHave.add(url); chrome.downloads.download({ url }).catch(() => {}); }
    else console.warn('YAD: add failed', err);
  }
});

// ---------------------------------------------------------------- tabs

chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.status === 'loading' && change.url) {
    ready.then(() => { const t = tabs.get(tabId); if (!t || t.pageUrl !== stripHash(change.url)) resetTab(tabId, change.url); });
  } else if (change.status === 'complete') {
    ready.then(() => { if (tabs.get(tabId)?.items.size) refreshTab(tabId); });
  }
  void tab;
});

chrome.tabs.onRemoved.addListener((tabId) => { tabs.delete(tabId); persist(); });

// ---------------------------------------------------------------- messages from popup / options / content

const handlers = {
  async getState() { await ready; return { ...publicState(), settings }; },
  async connect() {
    try { await native.connect({ force: true }); } catch { /* state carries the error */ }
    return { ...publicState(), settings };
  },
  /** "Start YAD": the host starts the service, then hello + getSettings (via onConnected). */
  async start() {
    try {
      await native.call('start', undefined);
      await native.connect({ force: true });
      await settingsSynced;
    } catch (err) {
      if (!native.state.error || native.connected) native.setState({ error: { code: 'start', text: err.message } });
    }
    return { ...publicState(), settings };
  },
  async refreshSettings() {
    await ready;
    if (await reachable(0)) { try { await syncSettings(); } catch { /* keep cache */ } }
    return { ...publicState(), settings };
  },
  async setBrowserSettings(msg) { await ready; return setBrowserSettings(msg.patch || {}); },
  async status() { return native.call('status'); },
  async getTab(msg) { await ready; return { pageUrl: tabs.get(msg.tabId)?.pageUrl || '', media: mediaView(msg.tabId), attachments: attachmentsView(msg.tabId) }; },
  async getMedia(_msg, sender) { await ready; return { items: sender.tab ? mediaView(sender.tab.id) : [] }; },
  async probe(msg) {
    // Probing on its own would start YAD; only do it while YAD is already running.
    if (!native.connected) return { ok: false, error: "YAD isn't running.", offline: true };
    try { return { ok: true, probe: await native.call('probe', { url: msg.url, headers: await headersFor(msg.url, msg.tabId) }, { timeout: 15_000 }) }; } catch (err) { return { ok: false, error: err.message, offline: !!err.offline }; }
  },
  add: addFromMessage,
  async addMany(msg, sender) {
    const results = [];
    for (const it of msg.items || []) results.push(await addFromMessage({ ...it, tabId: msg.tabId }, sender));
    return { results };
  },
  async altClick() { altUntil = Date.now() + ALT_BYPASS_MS; return true; },
};

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (sender.id !== chrome.runtime.id) return false;
  const h = msg && handlers[msg.type];
  if (!h) return false;
  Promise.resolve(h(msg, sender)).then(sendResponse, (err) => sendResponse({ ok: false, error: err?.message || String(err), offline: !!err?.offline }));
  return true;
});

// ---------------------------------------------------------------- lifecycle

chrome.runtime.onInstalled.addListener(() => { ready.then(updateContextMenu); });
chrome.runtime.onStartup.addListener(() => { ready.then(() => { updateContextMenu(); native.connect().catch(() => {}); }); });
ready.then(() => { updateContextMenu(); native.connect().catch(() => {}); });

export { NativeError };
