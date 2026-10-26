import { installSprite, icon, tile } from '../lib/icons.js';
import { esc, formatSize, fileNameFromUrl, hostOf } from '../lib/format.js';
import { KNOWN_EXTS, extOf, typeOf, tileFor } from '../lib/filetypes.js';

const RELEASES = 'https://github.com/innv8/yad/releases';
const MAX_FILES = 40;
const PROBE_FILES = 12;

const $body = document.getElementById('body');
const $status = document.getElementById('status');
const $statusText = document.getElementById('status-text');

let state = null;      // background state { connected, connecting, status, error, settings }
let tab = null;        // active tab
let rows = [];         // { key, section, url, variant, title, checked, ... }
let starting = false;
let startError = null;
let rendered = '';     // which view is on screen

const send = (msg) => chrome.runtime.sendMessage(msg);

// ------------------------------------------------------------ header status

function setStatus(kind, text) {
  $status.querySelector('.live').className = `live${kind === 'on' ? '' : kind === 'busy' ? ' busy' : ' off'}`;
  $statusText.textContent = text;
}

// ------------------------------------------------------------ offline view

function renderOffline() {
  rendered = 'offline';
  setStatus(starting ? 'busy' : 'off', starting ? 'Starting…' : 'Not running');
  const notInstalled = startError?.code === 'not-installed';
  const text = startError
    ? (notInstalled
      ? "YAD's browser link isn't set up. Open YAD once, or run <code>yadd install-native-host</code>, then try again."
      : `${esc(startError.text)} Try again, or open YAD from your applications.`)
    : 'Start YAD to send videos and files to it. Until then, downloads go to Chrome as usual.';
  $body.innerHTML = `<div class="offline">
    <div class="big">${icon('plug')}</div>
    <h3>YAD isn't running</h3>
    <p class="${startError ? 'err' : ''}">${text}</p>
    <button class="btn" id="start" style="margin-top:6px" ${starting ? 'disabled' : ''}>${starting ? '<span class="spin"></span>Starting YAD…' : 'Start YAD'}</button>
    <span class="get">Not installed? <a href="${RELEASES}" target="_blank" rel="noopener">Get YAD for your computer</a></span>
  </div>`;
  document.getElementById('start').addEventListener('click', start);
}

async function start() {
  starting = true;
  startError = null;
  renderOffline();
  const s = await send({ type: 'start' }).catch(() => null);
  starting = false;
  if (s?.connected) { state = s; await renderOnline(); return; }
  startError = s?.error || { code: 'offline', text: "YAD didn't start." };
  renderOffline();
}

// ------------------------------------------------------------ online view

/** Runs in the page: links to files and <video>/<audio> sources. Must be self-contained. */
function scanPage(exts) {
  const known = new Set(exts);
  const out = [];
  const seen = new Set();
  const add = (href, name) => {
    let u;
    try { u = new URL(href, location.href); } catch { return; }
    if (!/^https?:$/.test(u.protocol)) return;
    u.hash = '';
    if (seen.has(u.href)) return;
    seen.add(u.href);
    out.push({ url: u.href, name: (name || '').trim().slice(0, 200) });
  };
  for (const a of document.querySelectorAll('a[href]')) {
    let path = '';
    try { path = new URL(a.href, location.href).pathname; } catch { continue; }
    const m = /\.([a-z0-9]{1,8})$/i.exec(path);
    if ((m && known.has(m[1].toLowerCase())) || a.hasAttribute('download')) add(a.href, a.getAttribute('download') || '');
    if (out.length > 300) break;
  }
  for (const el of document.querySelectorAll('video[src], audio[src], video source[src], audio source[src]')) add(el.src, '');
  return { title: document.title, items: out };
}

async function scanTab() {
  if (!tab?.id || !/^https?:|^file:/.test(tab.url || '')) return [];
  try {
    const results = await chrome.scripting.executeScript({ target: { tabId: tab.id, allFrames: true }, func: scanPage, args: [KNOWN_EXTS] });
    return results.flatMap((r) => r.result?.items || []);
  } catch { return []; }
}

async function renderOnline() {
  rendered = 'online';
  setStatus('on', 'Connected');
  const [info, links] = await Promise.all([
    tab ? send({ type: 'getTab', tabId: tab.id }).catch(() => null) : null,
    scanTab(),
  ]);
  const media = info?.media || [];
  const title = tab?.title || '';
  rows = [];

  // Video on this tab: the preferred quality plus the audio-only track, per stream.
  for (const m of media.slice(0, 4)) {
    if (m.variants.length) {
      const pref = m.variants.find((v) => v.id === m.preferred) || m.variants[0];
      const audio = m.variants.find((v) => v.audioOnly);
      for (const v of [pref, audio !== pref ? audio : null].filter(Boolean)) {
        rows.push({ key: `${m.url}#${v.id}`, section: 'video', url: m.url, variant: v.id, title, name: title || m.fileName,
          badge: v.label, sub: [formatSize(v.size), v.container].filter(Boolean).join(' · '), tile: v.audioOnly ? 'audio' : 'video' });
      }
    } else {
      const isStream = m.kind !== 'file';
      rows.push({ key: m.url, section: 'video', url: m.url, variant: m.preferred || undefined, title,
        name: isStream ? (title || m.fileName) : (m.fileName || title), badge: isStream && !m.error ? (m.probing ? '…' : 'Best') : '',
        sub: m.error || [formatSize(m.size), m.container].filter(Boolean).join(' · ') || hostOf(m.url), tile: m.audio ? 'audio' : 'video', error: !!m.error });
    }
  }
  const first = rows.find((r) => !r.error);
  if (first) first.checked = true;

  // Files linked on this page (and attachments the page served).
  const mediaUrls = new Set(media.map((m) => m.url));
  const files = [];
  const seen = new Set();
  for (const f of [...(info?.attachments || []).map((a) => ({ url: a.url, name: a.fileName, size: a.size, contentType: a.contentType })), ...links]) {
    if (mediaUrls.has(f.url) || seen.has(f.url)) continue;
    seen.add(f.url);
    const name = f.name || fileNameFromUrl(f.url) || hostOf(f.url);
    const type = typeOf(extOf(name) || extOf(f.url), f.contentType);
    files.push({ key: f.url, section: 'file', url: f.url, name, size: f.size ?? null, sub: f.size != null ? formatSize(f.size) : hostOf(f.url), tile: tileFor(type) });
    if (files.length >= MAX_FILES) break;
  }
  rows.push(...files);

  const head = media.length === 1 && media[0].variants.length
    ? `${esc(media[0].label)} · ${media[0].variants.length} stream${media[0].variants.length === 1 ? '' : 's'}`
    : media.length > 1 ? `${media.length} found` : '';
  const videoButton = state?.settings?.browser?.videoButton !== false;

  const row = (r) => `<div class="prow" data-key="${esc(r.key)}">
      <button class="cb" role="checkbox" aria-checked="${r.checked ? 'true' : 'false'}" aria-label="${esc(r.name)}">${icon('check')}</button>
      ${tile(r.tile)}
      <div class="meta"><div class="name"><span class="t" title="${esc(r.url)}">${esc(r.name)}</span>${r.badge ? `<span class="q2">${esc(r.badge)}</span>` : ''}</div>
      <div class="sub${r.error ? ' err' : ''}" data-sub title="${esc(r.sub)}">${esc(r.sub)}</div></div></div>`;

  const videoRows = rows.filter((r) => r.section === 'video');
  $body.innerHTML = `${videoRows.length ? `<div class="psec"><span>Video on this tab</span><span>${head}</span></div>${videoRows.map(row).join('')}` : ''}
    ${files.length ? `<div class="psec"><span>Files linked on this page</span><button id="all" type="button">Select all</button></div><div class="list">${files.map(row).join('')}</div>` : ''}
    ${!rows.length ? `<div class="none"><b>Nothing to download here yet</b>Play a video, and the streams YAD can save appear here. Links to files on the page are listed too.</div>` : ''}
    <div class="msg" id="msg" hidden></div>
    <footer class="pfoot">
      <label><button class="toggle" id="vb" role="switch" aria-checked="${videoButton}"></button><span>Download button on videos</span></label>
      <button class="btn" id="go"></button>
    </footer>`;

  $body.querySelectorAll('.prow').forEach((el) => el.addEventListener('click', (e) => {
    const r = rows.find((x) => x.key === el.dataset.key);
    r.checked = !r.checked;
    el.querySelector('.cb').setAttribute('aria-checked', String(r.checked));
    updateGo();
    e.preventDefault();
  }));
  document.getElementById('all')?.addEventListener('click', () => {
    const fileRows = rows.filter((r) => r.section === 'file');
    const on = !fileRows.every((r) => r.checked);
    for (const r of fileRows) r.checked = on;
    $body.querySelectorAll('.list .cb').forEach((cb) => cb.setAttribute('aria-checked', String(on)));
    updateGo();
  });
  const vb = document.getElementById('vb');
  vb.addEventListener('click', async (e) => {
    e.preventDefault();
    const on = vb.getAttribute('aria-checked') !== 'true';
    vb.setAttribute('aria-checked', String(on));
    await send({ type: 'setBrowserSettings', patch: { videoButton: on } }).catch(() => {});
  });
  document.getElementById('go').addEventListener('click', downloadSelected);
  updateGo();
  probeSizes(files.filter((f) => f.size == null).slice(0, PROBE_FILES));
}

function updateGo() {
  const n = rows.filter((r) => r.checked).length;
  const go = document.getElementById('go');
  go.textContent = n ? `Download ${n}` : 'Download';
  go.disabled = !n;
}

async function probeSizes(files) {
  const queue = [...files];
  const worker = async () => {
    for (let f = queue.shift(); f; f = queue.shift()) {
      const res = await send({ type: 'probe', url: f.url, tabId: tab?.id }).catch(() => null);
      if (!res?.ok) { if (res?.offline) return; continue; }
      const el = $body.querySelector(`.prow[data-key="${CSS.escape(f.key)}"]`);
      if (res.probe.size != null && el) el.querySelector('[data-sub]').textContent = formatSize(res.probe.size);
    }
  };
  await Promise.all([worker(), worker(), worker()]);
}

async function downloadSelected() {
  const go = document.getElementById('go');
  const msg = document.getElementById('msg');
  const chosen = rows.filter((r) => r.checked);
  if (!chosen.length) return;
  go.disabled = true;
  go.innerHTML = '<span class="spin"></span>Sending…';
  const res = await send({
    type: 'addMany', tabId: tab?.id,
    items: chosen.map((r) => ({ url: r.url, variant: r.variant, title: r.title, pageUrl: tab?.url })),
  }).catch((e) => ({ results: [{ ok: false, error: e.message, offline: true }] }));
  const failed = (res?.results || []).filter((r) => !r.ok);
  if (!failed.length) {
    go.innerHTML = `${icon('check', 'style="width:14px;height:14px"')}Sent to YAD`;
    setTimeout(() => window.close(), 900);
    return;
  }
  if (failed.some((f) => f.offline)) { state = await send({ type: 'getState' }); renderOffline(); return; }
  msg.hidden = false;
  msg.textContent = failed.length === chosen.length ? failed[0].error : `${failed.length} of ${chosen.length} could not be sent: ${failed[0].error}`;
  updateGo();
}

// ------------------------------------------------------------ boot

async function render() {
  if (state?.connected) { if (rendered !== 'online') await renderOnline(); return; }
  if (state?.connecting && !starting) {
    setStatus('busy', 'Connecting…');
    if (rendered !== 'connecting') { rendered = 'connecting'; $body.innerHTML = '<div class="none"><span class="spin" style="display:inline-block"></span></div>'; }
    return;
  }
  if (!starting) renderOffline(); // matches the mockup; the specific reason shows after "Start YAD" fails
}

chrome.runtime.onMessage.addListener((msg) => {
  if (msg?.type === 'state') {
    if (starting) return;
    const was = state?.connected;
    state = { ...state, ...msg.state };
    if (was !== state.connected || (!state.connected && rendered === 'connecting')) render();
  } else if (msg?.type === 'tab' && msg.tabId === tab?.id && rendered === 'online') {
    // New media while the popup is open: re-render, keeping choices.
    const before = new Map(rows.map((r) => [r.key, !!r.checked]));
    renderOnline().then(() => {
      // Rows that were already there keep the user's choice; new rows keep their defaults.
      if (rows.some((r) => before.has(r.key))) for (const r of rows) if (before.has(r.key)) r.checked = before.get(r.key); else if (r.checked && [...before.values()].some(Boolean)) r.checked = false;
      $body.querySelectorAll('.prow').forEach((el) => el.querySelector('.cb').setAttribute('aria-checked', String(!!rows.find((r) => r.key === el.dataset.key)?.checked)));
      updateGo();
    });
  }
});

(async () => {
  installSprite();
  // ?tabId= lets the popup be opened in a normal tab for debugging.
  const forced = Number(new URLSearchParams(location.search).get('tabId'));
  tab = forced ? await chrome.tabs.get(forced).catch(() => null) : (await chrome.tabs.query({ active: true, currentWindow: true }))[0];
  state = await send({ type: 'getState' }).catch(() => null) || { connected: false };
  if (state.connected) state = await send({ type: 'refreshSettings' }).catch(() => state) || state;
  await render();
})();
