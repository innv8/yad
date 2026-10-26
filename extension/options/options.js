import { installSprite, icon } from '../lib/icons.js';
import { esc, normaliseSite, shortVersion, browserLabel } from '../lib/format.js';
import { HOST } from '../lib/native.js';

const RELEASES = 'https://github.com/innv8/yad/releases';
const SOURCE = 'https://github.com/innv8/yad';
const MIN_SIZES = [
  [0, 'Capture everything'], [1024 * 1024, '1 MB'], [5 * 1024 * 1024, '5 MB'], [10 * 1024 * 1024, '10 MB'], [50 * 1024 * 1024, '50 MB'],
];

const $main = document.getElementById('main');
let state = { connected: false, settings: null };
let section = 'capture';
let note = { text: '', err: false };

const send = (msg) => chrome.runtime.sendMessage(msg);
const b = () => state.settings?.browser || {};

// ------------------------------------------------------------ pieces

function statusBox({ withButton = false } = {}) {
  if (state.connected) {
    const v = state.status?.version ? ` ${esc(shortVersion(state.status.version))}` : '';
    return `<div class="statusbox"><span class="live"></span><div class="lab"><b>Connected to YAD${v}</b>
      <span>Through Chrome native messaging · no network port</span></div>
      ${withButton ? '<button class="btn ghost" data-act="reconnect">Reconnect</button>' : ''}</div>`;
  }
  if (state.connecting) {
    return `<div class="statusbox"><span class="live busy"></span><div class="lab"><b>Connecting to YAD…</b>
      <span>Starting YAD if it isn't running yet</span></div></div>`;
  }
  const why = state.error && state.error.code !== 'offline' ? state.error.text : 'Changes are kept here and sent to YAD when it starts.';
  return `<div class="statusbox"><span class="live off"></span><div class="lab"><b>YAD isn't running</b><span>${esc(why)}</span></div>
    <button class="btn" data-act="start">Start YAD</button></div>`;
}

const toggle = (key, title, desc) => `<div class="srow"><label class="lab" for="t-${key}"><b>${title}</b>${desc ? `<span>${desc}</span>` : ''}</label>
  <button class="toggle" id="t-${key}" role="switch" data-key="${key}" aria-checked="${b()[key] !== false}"></button></div>`;

function select(key, title, desc, options) {
  const value = b()[key];
  const opts = options.some(([v]) => v === value) ? options : [...options, [value, `${Math.round(value / 1024)} KB`]];
  return `<div class="srow"><label class="lab" for="s-${key}"><b>${title}</b>${desc ? `<span>${desc}</span>` : ''}</label>
    <div class="select"><select id="s-${key}" data-key="${key}">${opts.map(([v, l]) => `<option value="${v}" ${v === value ? 'selected' : ''}>${esc(l)}</option>`).join('')}</select>${icon('caret')}</div></div>`;
}

const noteHtml = () => `<div class="note ${note.err ? 'err' : ''}" id="note" role="status">${esc(note.text)}</div>`;

// ------------------------------------------------------------ sections

const sections = {
  capture: () => `<h2>Capture</h2><p class="lead">What the extension picks up and sends to YAD. These match Settings › Browser in the app.</p>
    ${statusBox()}
    ${toggle('videoButton', 'Show a Download button on videos', 'Detects direct files and streamed video (HLS, DASH) while it plays.')}
    ${toggle('takeOver', 'Take over browser downloads', 'Hold Alt while clicking a link to let Chrome download it instead.')}
    ${toggle('sendCookies', 'Send cookies with downloads', 'Needed for files behind a sign-in. Sent only to YAD on this computer.')}
    ${select('minSize', 'Skip files smaller than', '', MIN_SIZES)}
    ${toggle('contextMenu', 'Context menu', 'Adds “Download with YAD” to links and media.')}
    ${noteHtml()}`,

  sites: () => {
    const sites = b().excludedSites || [];
    return `<h2>Sites</h2><p class="lead">YAD leaves these sites alone: no Download button, and Chrome keeps their downloads.</p>
      ${sites.length ? sites.map((s) => `<div class="site">${icon('globe')}<span>${esc(s)}</span>
        <button class="icon-btn" data-remove="${esc(s)}" aria-label="Remove ${esc(s)}" title="Remove">${icon('x')}</button></div>`).join('')
        : '<div class="empty">No sites yet. YAD works on every site.</div>'}
      <form class="addsite" id="addsite"><input id="site" placeholder="docs.google.com" aria-label="Site to add" autocomplete="off" spellcheck="false">
        <button class="btn" type="submit">${icon('plus', 'style="width:14px;height:14px"')}Add site</button></form>
      ${noteHtml()}`;
  },

  connection: () => {
    const s = state.status;
    return `<h2>Connection</h2><p class="lead">YAD Link talks to YAD through Chrome native messaging. Nothing goes over the network.</p>
      ${statusBox({ withButton: true })}
      <dl class="kv">
        <dt>YAD</dt><dd>${s?.version ? esc(s.version) : '—'}</dd>
        <dt>Native host</dt><dd><code>${HOST}</code></dd>
        <dt>Extension ID</dt><dd><code>${esc(chrome.runtime.id)}</code></dd>
        <dt>Browser</dt><dd>${esc(browserLabel())}</dd>
      </dl>
      <h4>If YAD isn't found</h4>
      <ol class="steps">
        <li>Install YAD for your computer from <a href="${RELEASES}" target="_blank" rel="noopener">the releases page</a>.</li>
        <li>Open YAD once. It registers its browser link with Chrome.</li>
        <li>Or run <code>yadd install-native-host</code> in a terminal.</li>
        <li>Come back here and click <b>Start YAD</b>. If it still fails, restart Chrome.</li>
      </ol>
      <p class="text">Using a build of the extension from the Chrome Web Store or a different folder? Its extension ID must be listed in
        the native host manifest's <code>allowed_origins</code>; the installer adds the published IDs.</p>
      ${noteHtml()}`;
  },

  about: () => `<h2>About</h2><p class="lead">YAD Link ${esc(chrome.runtime.getManifest().version)} — the browser side of YAD, Yet Another Downloader.</p>
    <dl class="kv">
      <dt>Version</dt><dd>${esc(chrome.runtime.getManifest().version)}</dd>
      <dt>YAD</dt><dd>${state.status?.version ? esc(state.status.version) : 'not connected'}</dd>
      <dt>Source</dt><dd><a href="${SOURCE}" target="_blank" rel="noopener">github.com/innv8/yad</a></dd>
      <dt>Fonts</dt><dd>Archivo Narrow and Tenor Sans, SIL Open Font License</dd>
    </dl>`,
};

// ------------------------------------------------------------ rendering and actions

function render() {
  for (const a of document.querySelectorAll('.nav a')) {
    if (a.dataset.section === section) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
  }
  const focused = document.activeElement?.id;
  $main.innerHTML = sections[section]();
  if (focused) document.getElementById(focused)?.focus();
}

function setNote(text, err = false) {
  note = { text, err };
  const el = document.getElementById('note');
  if (el) { el.textContent = text; el.className = `note ${err ? 'err' : ''}`; }
}

async function save(patch) {
  state.settings = { ...state.settings, browser: { ...b(), ...patch } };
  try {
    const res = await send({ type: 'setBrowserSettings', patch });
    if (res?.settings) state.settings = res.settings;
    setNote(res?.synced === false ? 'Saved. YAD gets the change when it starts.' : 'Saved');
  } catch (err) {
    setNote(err.message || 'Could not save', true);
  }
}

$main.addEventListener('click', async (e) => {
  const t = e.target.closest('button');
  if (!t) return;
  if (t.matches('.toggle')) {
    const on = t.getAttribute('aria-checked') !== 'true';
    t.setAttribute('aria-checked', String(on));
    await save({ [t.dataset.key]: on });
  } else if (t.dataset.remove) {
    await save({ excludedSites: (b().excludedSites || []).filter((s) => s !== t.dataset.remove) });
    render();
  } else if (t.dataset.act === 'reconnect' || t.dataset.act === 'start') {
    state.connecting = true;
    render();
    const s = await send({ type: t.dataset.act === 'start' ? 'start' : 'connect' }).catch(() => null);
    if (s) state = s;
    state.connecting = false;
    render();
  }
});

$main.addEventListener('change', (e) => {
  if (e.target.matches('select[data-key]')) save({ [e.target.dataset.key]: Number(e.target.value) });
});

$main.addEventListener('submit', async (e) => {
  if (e.target.id !== 'addsite') return;
  e.preventDefault();
  const input = document.getElementById('site');
  const site = normaliseSite(input.value);
  if (!site) { setNote('Type a site like example.com', true); return; }
  const sites = b().excludedSites || [];
  if (sites.includes(site)) { setNote(`${site} is already on the list`); return; }
  await save({ excludedSites: [...sites, site] });
  render();
  document.getElementById('site')?.focus();
});

function route() {
  const s = location.hash.slice(1);
  section = sections[s] ? s : 'capture';
  note = { text: '', err: false };
  render();
}
addEventListener('hashchange', route);

chrome.runtime.onMessage.addListener((msg) => {
  if (msg?.type === 'state') {
    const was = [state.connected, state.connecting, state.status?.version];
    state = { ...state, ...msg.state };
    if (was.join() !== [state.connected, state.connecting, state.status?.version].join()) render();
  }
  if (msg?.type === 'settings' && !document.activeElement?.matches('input')) { state.settings = msg.settings; render(); }
});

(async () => {
  installSprite();
  state = await send({ type: 'getState' }).catch(() => null) || state;
  route();
  if (state.connected) {
    const s = await send({ type: 'refreshSettings' }).catch(() => null);
    if (s) { state = s; render(); }
  }
})();
