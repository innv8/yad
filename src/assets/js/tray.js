// Tray / menu-bar popover (tray.html, 320px wide, no title bar).
import { setTheme } from './theme.js';
import { call, bridge, onEvent, listenConnection, onConnection } from './api.js';
import { esc, bytes, speed, etaShort, pct, extractUrls, MOD, modKey, errText } from './format.js';
import { injectSprite } from './icons.js';

injectSprite();
const $ = (id) => document.getElementById(id);
$('quit-kbd').textContent = `${MOD} Q`;

const downloads = new Map();
let connected = true;
let noteTimer = null;

function note(text) {
  const n = $('tray-note');
  n.textContent = text;
  n.hidden = !text;
  clearTimeout(noteTimer);
  if (text) noteTimer = setTimeout(() => { n.hidden = true; }, 4000);
}

function render() {
  const all = [...downloads.values()];
  const active = all.filter((d) => d.status === 'Downloading').sort((a, b) => (b.speed || 0) - (a.speed || 0));
  const total = active.reduce((s, d) => s + (d.speed || 0), 0);
  const queued = all.filter((d) => d.status === 'Queued').length;
  $('tray-summary').textContent = !connected ? 'Service not reachable'
    : active.length ? `${active.length} downloading · ${speed(total)}`
      : queued ? `${queued} waiting` : 'Nothing downloading';
  const rows = active.slice(0, 3).map((d) => {
    const p = pct(d);
    const sub = [speed(d.speed), d.eta != null ? etaShort(d.eta) : bytes(d.downloaded)].join(' · ');
    return `<div class="trow"><div class="top2"><b>${esc(d.fileName)}</b><span>${p == null ? '' : `${p}%`}</span></div><div class="track"><div class="fill" data-w="${p ?? 0}"></div></div><div class="sub">${esc(sub)}</div></div>`;
  }).join('');
  const html = rows || '';
  const box = $('tray-rows');
  if (box._html !== html) {
    box.innerHTML = html;
    box._html = html;
    for (const f of box.querySelectorAll('.fill')) f.style.width = `${f.dataset.w}%`;
  }
  const paused = all.some((d) => d.status === 'Paused');
  const pa = document.querySelector('[data-a="pauseAll"] span');
  const label = !active.length && !queued && paused ? 'Resume all' : 'Pause all';
  if (pa.textContent !== label) pa.textContent = label;
}

async function load() {
  try {
    const [list, settings] = await Promise.all([call('list'), call('getSettings')]);
    downloads.clear();
    for (const d of list || []) downloads.set(d.id, d);
    if (settings) setTheme(settings.theme);
  } catch (e) { note(errText(e)); }
  render();
}

onEvent((ev) => {
  if (!ev) return;
  if (ev.type === 'added' || ev.type === 'updated') downloads.set(ev.download.id, { ...downloads.get(ev.download.id), ...ev.download });
  else if (ev.type === 'removed') downloads.delete(ev.id);
  else if (ev.type === 'progress') {
    for (const it of ev.items || []) { const d = downloads.get(it.id); if (d) Object.assign(d, it); }
  } else if (ev.type === 'settings') setTheme(ev.settings.theme);
  render();
});
onConnection((c) => { connected = c; render(); if (c) load(); });
listenConnection();
const setAnchor = (edge) => document.body.classList.toggle('anchor-bottom', edge === 'bottom');
window.__TAURI__.event.listen('tray-anchor', (e) => setAnchor(e.payload && e.payload.edge));

async function act(a) {
  try {
    if (a === 'pauseAll') {
      const label = document.querySelector('[data-a="pauseAll"] span').textContent;
      await call(label === 'Resume all' ? 'resumeAll' : 'pauseAll');
    } else if (a === 'clipboard') {
      const cm = window.__TAURI__ && window.__TAURI__.clipboardManager;
      const text = cm && cm.readText ? await cm.readText() : await navigator.clipboard.readText();
      const urls = extractUrls(text);
      if (!urls.length) { note('No link on the clipboard'); return; }
      for (const url of urls) await call('add', { url, source: 'clipboard' });
    } else if (a === 'open') await bridge('show_main');
    else if (a === 'quit') await bridge('quit_app', { stopDaemon: false });
  } catch (e) { note(errText(e)); }
}

$('tray').addEventListener('click', (e) => {
  const b = e.target.closest('[data-a]');
  if (b) act(b.dataset.a);
});
document.addEventListener('keydown', (e) => {
  if (modKey(e) && (e.key === 'q' || e.key === 'Q')) { e.preventDefault(); act('quit'); }
});
document.addEventListener('contextmenu', (e) => e.preventDefault());

load();
