// Main window: wires state, daemon events and the views together.
import { setTheme } from './theme.js';
import { call, bridge, onEvent, listenConnection, onConnection } from './api.js';
import { state, on, emit, applyEvent, setDownloads } from './state.js';
import { MOD, modKey, extractUrls } from './format.js';
import { injectSprite } from './icons.js';
import { initTooltips } from './tooltip.js';
import { setToastHost, showToast, flushToast } from './toast.js';
import { setSheetHost, isSheetOpen } from './sheet.js';
import { isMenuOpen } from './menu.js';
import { initTitlebar, setTitle } from './titlebar.js';
import { initSidebar } from './sidebar.js';
import { initList, moveSelection, select, startRename } from './list.js';
import { initDetails, closeDrawer } from './details.js';
import { initSettings, renderSettings } from './settings.js';
import * as A from './actions.js';

const $ = (id) => document.getElementById(id);
const titlebar = $('titlebar');
const pasteInput = $('paste-input');

injectSprite();
initTooltips();
initTitlebar(titlebar);
setToastHost($('view'));
setSheetHost($('app'));
$('paste-kbd').textContent = `${MOD} V`;

// ---------- views ----------
function showSettings(page) {
  if (page) state.settingsPage = page;
  state.view = 'settings';
  $('downloads').hidden = true;
  $('settings').hidden = false;
  setTitle(titlebar, 'Settings');
  emit('view');
  renderSettings();
  $('settings').scrollTop = 0;
}
function showDownloads() {
  state.view = 'downloads';
  $('downloads').hidden = false;
  $('settings').hidden = true;
  setTitle(titlebar, '');
  emit('view');
}

initSidebar($('sidebar'), { onSettingsPage: showSettings, onBack: showDownloads });
initList($('main'));
initDetails($('drawer'), $('downloads'));
initSettings($('settings'));
$('open-settings').addEventListener('click', () => showSettings());

on('settings', () => { if (state.settings) setTheme(state.settings.theme); });

// ---------- data ----------
let loading = null;
async function load() {
  if (loading) return loading;
  loading = (async () => {
    const [list, settings, status] = await Promise.allSettled([call('list'), call('getSettings'), call('status')]);
    if (settings.status === 'fulfilled') { state.settings = settings.value; emit('settings'); }
    if (status.status === 'fulfilled') { state.status = status.value; emit('status'); }
    if (list.status === 'fulfilled') setDownloads(list.value);
  })().finally(() => { loading = null; });
  return loading;
}

async function refreshStatus() {
  try { state.status = await call('status'); emit('status'); } catch { /* banner shows it */ }
}

function setConnected(c) {
  state.connected = c;
  $('banner').hidden = c;
  emit('connection');
  if (c) load();
}
onConnection(setConnected);

// ---------- adding links ----------
function addFromText(text, source) {
  const urls = extractUrls(text);
  if (!urls.length) return false;
  A.addUrls(urls, source);
  return true;
}

function submitPasteBox() {
  let v = pasteInput.value.trim();
  if (!v) return;
  if (!/^[a-z]+:\/\//i.test(v) && /^[\w-]+(\.[\w-]+)+(\/|$)/.test(v)) v = `https://${v}`;
  if (addFromText(v, 'app')) pasteInput.value = '';
  else showToast("That doesn't look like a link", { duration: 3000 });
}
pasteInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.preventDefault(); submitPasteBox(); }
  else if (e.key === 'Escape') { pasteInput.value = ''; pasteInput.blur(); }
});
pasteInput.addEventListener('focus', () => $('paste').classList.add('focus'));
pasteInput.addEventListener('blur', () => $('paste').classList.remove('focus'));

const isEditable = (el) => !!(el && el.closest && el.closest('input, textarea, [contenteditable]:not([contenteditable="false"])'));

document.addEventListener('paste', (e) => {
  if (isSheetOpen()) return;
  const t = document.activeElement;
  const inBox = t === pasteInput;
  if (isEditable(t) && !inBox) return; // another input: normal paste
  if (inBox && pasteInput.value.trim()) return; // editing a link by hand
  const text = (e.clipboardData && (e.clipboardData.getData('text/uri-list') || e.clipboardData.getData('text/plain'))) || '';
  if (addFromText(text, 'clipboard')) {
    e.preventDefault();
    pasteInput.value = '';
    if (state.view !== 'downloads') showDownloads();
  } else if (!inBox && text.trim()) {
    showToast("That doesn't look like a link", { duration: 3000 });
  }
});

// drag & drop of links / text/uri-list / .txt lists
let dragDepth = 0;
const hint = document.createElement('div');
hint.className = 'drop-hint';
hint.textContent = 'Drop to download';
hint.hidden = true;
$('app').appendChild(hint);
const droppable = (dt) => dt && [...dt.types].some((t) => t === 'text/uri-list' || t === 'text/plain' || t === 'Files');
window.addEventListener('dragenter', (e) => {
  if (!droppable(e.dataTransfer) || isSheetOpen()) return;
  e.preventDefault();
  dragDepth++;
  hint.hidden = false;
});
window.addEventListener('dragover', (e) => {
  if (!droppable(e.dataTransfer)) return;
  e.preventDefault();
  e.dataTransfer.dropEffect = 'copy';
});
window.addEventListener('dragleave', () => {
  dragDepth = Math.max(0, dragDepth - 1);
  if (!dragDepth) hint.hidden = true;
});
window.addEventListener('drop', async (e) => {
  e.preventDefault();
  dragDepth = 0;
  hint.hidden = true;
  if (isSheetOpen()) return;
  const dt = e.dataTransfer;
  let text = '';
  const uri = dt.getData('text/uri-list');
  if (uri) text = uri.split(/\r?\n/).filter((l) => l && !l.startsWith('#')).join('\n');
  if (!text) text = dt.getData('text/plain');
  if (!text && dt.files && dt.files.length) {
    const parts = await Promise.all([...dt.files]
      .filter((f) => /\.(txt|list|urls)$/i.test(f.name) || f.type === 'text/plain' || f.type === 'text/uri-list')
      .map((f) => f.text().catch(() => '')));
    text = parts.join('\n');
  }
  if (!addFromText(text, 'app')) showToast('No links found in what you dropped', { duration: 3000 });
  else if (state.view !== 'downloads') showDownloads();
});

// ---------- keyboard ----------
document.addEventListener('keydown', (e) => {
  if (isSheetOpen() || isMenuOpen() || e.defaultPrevented) return;
  if (modKey(e) && !e.shiftKey && !e.altKey) {
    if (e.key === ',') { e.preventDefault(); showSettings(); return; }
    if (e.key === 'q' || e.key === 'Q') { e.preventDefault(); bridge('quit_app', { stopDaemon: false }); return; }
    if (e.key === 'l' || e.key === 'L') { e.preventDefault(); showDownloads(); pasteInput.focus(); return; }
  }
  if (isEditable(e.target)) return;
  if (state.view === 'settings') {
    if (e.key === 'Escape') { e.preventDefault(); showDownloads(); }
    return;
  }
  const onButton = e.target.closest && e.target.closest('button');
  const d = state.downloads.get(state.selected);
  switch (e.key) {
    case 'ArrowDown': e.preventDefault(); moveSelection(1); break;
    case 'ArrowUp': e.preventDefault(); moveSelection(-1); break;
    case 'Enter': if (!onButton && d) { e.preventDefault(); A.enter(d); } break;
    case ' ': if (!onButton && d) { e.preventDefault(); A.togglePause(d); } break;
    case 'Delete':
    case 'Backspace': if (d) { e.preventDefault(); A.removeWithUndo([d]); } break;
    case 'F2': if (d) { e.preventDefault(); startRename(d.id); } break;
    case 'ContextMenu':
    case 'F10':
      if (d && (e.key === 'ContextMenu' || e.shiftKey)) {
        e.preventDefault();
        const btn = document.querySelector(`.row[data-id="${d.id}"] [data-act="more"]`);
        if (btn) A.rowMenu(d, { anchor: btn, keyboard: true });
      }
      break;
    case 'Escape':
      if (state.drawerId != null) closeDrawer();
      else if (state.selected != null) select(null);
      break;
    default: break;
  }
});

// No native context menu anywhere (rows have their own).
document.addEventListener('contextmenu', (e) => { if (!isEditable(e.target)) e.preventDefault(); });

window.addEventListener('beforeunload', flushToast);

// ---------- start ----------
onEvent(applyEvent);
listenConnection();
load();
setInterval(refreshStatus, 10000);
