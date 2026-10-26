// Settings view. Every control writes through setSettings immediately (optimistic, then the
// daemon's answer wins).
import { state, on, emit } from './state.js';
import { call, pickFolder, openPath } from './api.js';
import { esc, bytes, speed, tildify, ago } from './format.js';
import { icon } from './icons.js';
import { openMenu } from './menu.js';
import { toastError } from './toast.js';

export const PAGES = [
  ['general', 'gear', 'General'],
  ['downloads', 'gauge', 'Downloads'],
  ['browser', 'globe', 'Browser'],
  ['appearance', 'paint', 'Appearance'],
  ['about', 'info', 'About'],
];
export const REPO_URL = 'https://github.com/innv8/yad';

const MB = 1024 * 1024;
const SPEED_LIMITS = [null, 256 * 1024, 512 * 1024, 1 * MB, 2 * MB, 5 * MB, 10 * MB, 25 * MB, 50 * MB];
const RETRIES = [0, 1, 3, 5, 10];
const QUALITIES = [['best', 'Best available'], ['1080', '1080p'], ['720', '720p'], ['480', '480p'], ['audio', 'Audio only']];
const MIN_SIZES = [0, 100 * 1024, 512 * 1024, 1 * MB, 5 * MB, 10 * MB, 50 * MB];
const CAPTURE_TYPES = [['Videos', 'Video'], ['Audio', 'Audio'], ['Compressed', 'Archives'], ['Programs', 'Programs'], ['Documents', 'Documents'], ['Images', 'Images']];

const limitLabel = (v) => (v ? speed(v) : 'No limit');
const retryLabel = (n) => (n === 0 ? "Don't retry" : n === 1 ? 'Once' : `${n} times`);
const sizeLabel = (n) => (n ? bytes(n) : 'No minimum');

let el = null;
let addingSite = false;

const srow = (title, desc, ctl, key = '') => `<div class="srow"${key ? ` data-row="${key}"` : ''}><div class="lab"><b>${title}</b>${desc ? `<span>${desc}</span>` : ''}</div>${ctl}</div>`;
const tg = (key, on, extra = '') => `<button class="toggle${on ? ' on' : ''}" role="switch" aria-checked="${on}" data-toggle="${key}" data-focus="${key}"${extra}></button>`;
const sel = (key, label) => `<button class="select" data-select="${key}" data-focus="${key}"><span>${esc(label)}</span>${icon('caret')}</button>`;
const step = (key, v, min, max) => `<div class="stepper"><button data-step="${key}" data-d="-1" data-focus="${key}-" aria-label="Fewer"${v <= min ? ' disabled' : ''}>−</button><b>${v}</b><button data-step="${key}" data-d="1" data-focus="${key}+" aria-label="More"${v >= max ? ' disabled' : ''}>+</button></div>`;

function extBox() {
  const ext = (state.status && state.status.extension) || {};
  const browser = ext.browser || 'Chrome';
  if (ext.connected) {
    const bits = [`YAD Link${ext.version ? ` ${ext.version}` : ''}`, browser, `last used ${ago(ext.lastSeen)}`];
    return `<div class="statusbox"><span class="live"></span><div class="lab"><b>${esc(browser.split(/\s/)[0])} extension connected</b><span>${esc(bits.join(' · '))}</span></div><button class="btn ghost" data-a="ext-help">Reinstall</button></div>`;
  }
  return `<div class="statusbox"><span class="live off"></span><div class="lab"><b>Extension not linked</b><span>Install YAD Link in Chrome, then open it once to connect.</span></div><button class="btn ghost" data-a="ext-help">Get extension</button></div>`;
}

function page(s) {
  const st = state.status || {};
  switch (state.settingsPage) {
    case 'general':
      return `<h2>General</h2><p class="lead">Where files go and how YAD runs on this computer.</p>
        ${srow('Download folder', 'Files are saved here unless you pick another folder.', `<button class="select folder" data-a="folder" data-focus="folder"><span>${icon('folder')}<span class="path">${esc(tildify(s.downloadDir))}</span></span><span class="change">Change</span></button>`)}
        ${srow('Sort into folders by type', 'Videos, Documents, Programs and so on.', tg('sortByType', s.sortByType))}
        ${srow('Ask before each download', 'Show name and folder before starting.', tg('askBeforeDownload', s.askBeforeDownload))}
        <h4>Background</h4>
        ${srow('Keep downloading when the window is closed', 'YAD keeps running in the tray or menu bar.', tg('keepRunning', s.keepRunning))}
        ${srow('Start at login', 'Resume unfinished downloads after a restart.', tg('startAtLogin', s.startAtLogin))}
        ${srow('Notify when a download finishes', '', tg('notifications', s.notifications))}`;
    case 'downloads':
      return `<h2>Downloads</h2><p class="lead">How hard YAD works on each file.</p>
        ${srow('Downloads at the same time', 'Others wait in the queue.', step('maxActive', s.maxActive, 1, 10))}
        ${srow('Connections per download', 'More parts help on fast lines. Some servers limit this.', step('connectionsPerDownload', s.connectionsPerDownload, 1, 16))}
        ${srow('Speed limit', 'Applies to all downloads together.', sel('speedLimit', limitLabel(s.speedLimit)))}
        ${srow('Retry when a connection drops', 'Waits 1, 2, 4, 8 then 16 seconds.', sel('retries', retryLabel(s.retries)))}
        <h4>Videos</h4>
        ${srow('Preferred video quality', 'Used when you click Download without choosing.', sel('videoQuality', (QUALITIES.find((x) => x[0] === s.videoQuality) || QUALITIES[0])[1]))}
        ${srow('Join audio and video tracks', st.ffmpeg === null && state.status ? '<span class="warn">Needs ffmpeg, which was not found on this computer.</span>' : 'For sites that stream them separately.', tg('mergeTracks', s.mergeTracks))}`;
    case 'browser': {
      const b = s.browser || {};
      const types = b.types || [];
      const sites = b.excludedSites || [];
      return `<h2>Browser</h2><p class="lead">Let Chrome hand downloads to YAD.</p>
        ${extBox()}
        ${srow('Show a Download button on videos', 'Appears over a video as soon as it starts playing.', tg('browser.videoButton', b.videoButton))}
        ${srow('Take over browser downloads', 'Files Chrome would download go to YAD instead.', tg('browser.takeOver', b.takeOver))}
        ${srow('Only for these types', '', `<div class="chips">${CAPTURE_TYPES.map(([t, l]) => `<button class="${types.includes(t) ? 'on' : ''}" data-type="${t}" data-focus="type-${t}" aria-pressed="${types.includes(t)}">${l}</button>`).join('')}</div>`)}
        ${srow('Skip files smaller than', 'Small files stay in Chrome.', sel('browser.minSize', sizeLabel(b.minSize)))}
        ${srow('Never capture on', '', `<div class="chips">${sites.map((h) => `<button data-site="${esc(h)}" data-tip="Remove" data-focus="site-${esc(h)}">${esc(h)}${icon('x')}</button>`).join('')}${addingSite ? '<span class="chip-in"><input class="site-in" placeholder="example.com" spellcheck="false"></span>' : '<button class="add" data-a="add-site" data-focus="add-site">+ Add site</button>'}</div>`)}
        ${srow('Send cookies with downloads', 'Needed for files behind a sign-in. Sent only to YAD on this computer.', tg('browser.sendCookies', b.sendCookies))}
        ${srow('Context menu', 'Adds “Download with YAD” to links and media.', tg('browser.contextMenu', b.contextMenu))}`;
    }
    case 'appearance':
      return `<h2>Appearance</h2><p class="lead">How YAD looks.</p>
        ${srow('Theme', 'System follows your computer’s light or dark setting.', `<div class="segctl">${[['system', 'System'], ['light', 'Light'], ['dark', 'Dark']].map(([v, l]) => `<button class="${s.theme === v ? 'on' : ''}" data-theme-set="${v}" data-focus="theme-${v}" aria-pressed="${s.theme === v}">${l}</button>`).join('')}</div>`)}`;
    case 'about':
      return `<h2>About</h2><p class="lead">Yet Another Downloader.</p>
        <div class="about-mark"><div class="logo">${icon('down')}</div><div><b>YAD${st.version ? ` ${esc(st.version)}` : ''}</b><span>Fast, resumable downloads for your desktop.</span></div></div>
        ${srow('Source code', 'Free and open source.', `<button class="link-btn" data-a="repo" data-focus="repo">github.com/innv8/yad</button>`)}
        ${srow('Background service', st.pid ? `Running as process ${esc(st.pid)}.` : 'Not reachable.', `<span class="val">${st.serviceInstalled ? 'Starts at login' : 'Starts with the app'}</span>`)}
        ${srow('ffmpeg', 'Used to join separate audio and video tracks.', `<span class="val" data-tip="${esc(st.ffmpeg || '')}">${st.ffmpeg ? esc(tildify(st.ffmpeg)) : 'Not found'}</span>`)}`;
    default:
      return '';
  }
}

export function renderSettings() {
  if (!el || state.view !== 'settings') return;
  const s = state.settings;
  if (!s) { el.innerHTML = '<p class="lead">Loading…</p>'; return; }
  const focusKey = document.activeElement && el.contains(document.activeElement) ? document.activeElement.dataset.focus : null;
  const wasSiteInput = document.activeElement && document.activeElement.classList.contains('site-in');
  el.innerHTML = page(s);
  if (focusKey) { const f = el.querySelector(`[data-focus="${CSS.escape(focusKey)}"]`); if (f) f.focus(); }
  const si = el.querySelector('.site-in');
  if (si && (addingSite || wasSiteInput)) si.focus();
}

function deepSet(obj, path, v) {
  const out = { ...obj };
  if (path.length === 1) { out[path[0]] = v; return out; }
  out[path[0]] = deepSet(obj[path[0]] || {}, path.slice(1), v);
  return out;
}

/** setSettings with a dotted key ("browser.takeOver"). */
export async function setSetting(key, value) {
  const path = key.split('.');
  const before = state.settings;
  state.settings = deepSet(state.settings, path, value);
  emit('settings');
  const partial = deepSet({}, path, value);
  try {
    const s = await call('setSettings', partial);
    if (s && typeof s === 'object') { state.settings = s; emit('settings'); }
  } catch (e) {
    state.settings = before;
    emit('settings');
    toastError(e);
  }
}

const get = (key) => key.split('.').reduce((o, k) => (o ? o[k] : undefined), state.settings);

function openSelect(btn, key, ev) {
  const cur = get(key);
  const opts = {
    speedLimit: SPEED_LIMITS.map((v) => [v, limitLabel(v)]),
    retries: RETRIES.map((v) => [v, retryLabel(v)]),
    videoQuality: QUALITIES,
    'browser.minSize': MIN_SIZES.map((v) => [v, sizeLabel(v)]),
  }[key];
  if (!opts) return;
  if (btn._skipClick) { btn._skipClick = false; return; }
  openMenu({
    anchor: btn,
    width: Math.max(160, btn.getBoundingClientRect().width),
    keyboard: ev.detail === 0,
    items: opts.map(([v, l]) => ({ label: l, checked: v === cur, action: () => setSetting(key, v) })),
  });
}

// The daemon installs/uninstalls the login service itself when startAtLogin changes.
async function setStartAtLogin(on) {
  await setSetting('startAtLogin', on);
  try { state.status = await call('status'); emit('status'); } catch { /* banner handles it */ }
}

function commitSite(input) {
  const v = input.value.trim().toLowerCase().replace(/^[a-z]+:\/\//, '').replace(/\/.*$/, '');
  addingSite = false;
  const sites = (state.settings.browser && state.settings.browser.excludedSites) || [];
  if (v && /^[a-z0-9.-]+(:\d+)?$/.test(v) && !sites.includes(v)) setSetting('browser.excludedSites', [...sites, v]);
  else renderSettings();
}

export function initSettings(mainEl) {
  el = mainEl;
  el.addEventListener('click', (e) => {
    const t = e.target.closest('button');
    if (!t || !state.settings) return;
    if (t.dataset.toggle) {
      const key = t.dataset.toggle;
      const v = !get(key);
      if (key === 'startAtLogin') setStartAtLogin(v); else setSetting(key, v);
    } else if (t.dataset.select) openSelect(t, t.dataset.select, e);
    else if (t.dataset.step) {
      const key = t.dataset.step;
      const lim = key === 'maxActive' ? [1, 10] : [1, 16];
      const v = Math.max(lim[0], Math.min(lim[1], (get(key) || 1) + Number(t.dataset.d)));
      if (v !== get(key)) setSetting(key, v);
    } else if (t.dataset.type) {
      const types = (state.settings.browser && state.settings.browser.types) || [];
      const ty = t.dataset.type;
      setSetting('browser.types', types.includes(ty) ? types.filter((x) => x !== ty) : [...types, ty]);
    } else if (t.dataset.site != null) {
      const sites = state.settings.browser.excludedSites || [];
      setSetting('browser.excludedSites', sites.filter((x) => x !== t.dataset.site));
    } else if (t.dataset.themeSet) setSetting('theme', t.dataset.themeSet);
    else if (t.dataset.a === 'folder') {
      pickFolder(state.settings.downloadDir).then((dir) => { if (dir) setSetting('downloadDir', dir); }, toastError);
    } else if (t.dataset.a === 'add-site') { addingSite = true; renderSettings(); }
    else if (t.dataset.a === 'repo' || t.dataset.a === 'ext-help') openPath(REPO_URL).catch(toastError);
  });
  el.addEventListener('keydown', (e) => {
    if (!e.target.classList.contains('site-in')) return;
    e.stopPropagation();
    if (e.key === 'Enter') commitSite(e.target);
    else if (e.key === 'Escape') { addingSite = false; renderSettings(); }
  });
  el.addEventListener('focusout', (e) => {
    if (e.target.classList && e.target.classList.contains('site-in') && addingSite) setTimeout(() => { if (addingSite) commitSite(e.target); }, 0);
  });
  on('settings', renderSettings);
  on('status', () => { if (['browser', 'about', 'downloads'].includes(state.settingsPage)) renderSettings(); });
}
