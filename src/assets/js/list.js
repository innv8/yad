// Download list: grouped rows, reconciled by id so progress/added/updated/removed patch in place.
import { state, on, emit, visibleDownloads, filterMatch, filterLabel } from './state.js';
import { esc, bytes, speed, etaShort, pct, dayBucket, whenLabel, host, MOD } from './format.js';
import { icon, tile } from './icons.js';
import { call } from './api.js';
import { toastError } from './toast.js';
import * as A from './actions.js';
import { openDrawer } from './details.js';

let els = {};
const rowEls = new Map();   // id → row element
const groupEls = new Map(); // key → group header element
let nothingEl = null;

const GROUPS = { queue: 'Queue', today: 'Today', yesterday: 'Yesterday', earlier: 'Earlier' };

function browserName() {
  const b = state.status && state.status.extension && state.status.extension.browser;
  const n = b ? String(b).split(/[\s/]+/)[0] : '';
  return n || 'Chrome';
}
const SEP = '<span class="sep">·</span>';
const chip = (d) => (d.source === 'extension' ? `${SEP}<span class="chip">${icon('globe')}from ${esc(browserName())}</span>` : '');
const ofTotal = (d) => (d.totalSize ? `${bytes(d.downloaded)} of ${bytes(d.totalSize)}` : bytes(d.downloaded));

function subHtml(d) {
  const parts = [];
  switch (d.status) {
    case 'Downloading':
      parts.push(ofTotal(d), speed(d.speed));
      if (d.eta != null) parts.push(etaShort(d.eta));
      if (d.connections > 1) parts.push(`${d.connections} connections`);
      break;
    case 'Queued':
      parts.push(d.downloaded ? ofTotal(d) : d.totalSize ? bytes(d.totalSize) : 'Size unknown', 'waiting for a free slot');
      break;
    case 'Paused':
      parts.push(`Paused at ${ofTotal(d)}`, d.resumable ? 'resumes where it stopped' : 'starts over when resumed');
      break;
    case 'Failed':
    case 'Cancelled': {
      const what = d.status === 'Cancelled' ? 'Cancelled' : (d.error || 'Failed');
      parts.push(d.downloaded ? `${what} at ${ofTotal(d)}` : what);
      if (d.retries > 0) parts.push(`retried ${d.retries}×`);
      break;
    }
    case 'Completed':
      parts.push(bytes(d.totalSize ?? d.downloaded));
      if (host(d.url)) parts.push(host(d.url));
      if (d.finishedAt) parts.push(whenLabel(d.finishedAt));
      break;
    default:
      parts.push(ofTotal(d));
  }
  return parts.map((p) => esc(p)).join(SEP) + chip(d);
}

const hasTrack = (d) => d.status !== 'Completed';
const fillCls = (d) => ({ Paused: 'paused', Failed: 'err', Cancelled: 'err', Queued: 'idle' }[d.status] || '');
const pill = (d) => ({
  Failed: '<span class="pill err">Failed</span>',
  Cancelled: '<span class="pill mut">Cancelled</span>',
  Completed: '<span class="pill ok">Done</span>',
}[d.status] || '');

const shapeSig = (d) => [d.status, d.fileName, d.fileType, d.source].join('|');

function rowInner(d) {
  const p = pill(d);
  const lead = p || `<span class="pct"></span>`;
  return `${tile(d.fileType)}<div class="meta"><div class="name">${esc(d.fileName)}</div><div class="sub"></div>${hasTrack(d) ? `<div class="track"><div class="fill ${fillCls(d)}"></div></div>` : ''}</div>`
    + `<div class="end">${lead}<button class="icon-btn" data-act="primary" data-tip="${esc(A.primaryLabel(d))}" aria-label="${esc(A.primaryLabel(d))}">${icon(A.primaryIcon(d))}</button>`
    + `<button class="icon-btn" data-act="more" data-tip="More" aria-label="More">${icon('more')}</button></div>`;
}

function patchRow(el, d) {
  const sig = shapeSig(d);
  if (el._sig !== sig) {
    if (el._renaming) return; // keep the rename input alive
    el._sig = sig;
    el.innerHTML = rowInner(d);
    el._sub = el.querySelector('.sub');
    el._fill = el.querySelector('.fill');
    el._pct = el.querySelector('.pct');
    el._prim = el.querySelector('[data-act="primary"]');
    el._more = el.querySelector('[data-act="more"]');
    el._subHtml = el._w = el._p = null;
  }
  // Selection / open-menu state only toggles classes, so buttons are never replaced under the pointer.
  const sel = state.selected === d.id;
  el.classList.toggle('sel', sel);
  el.setAttribute('aria-selected', sel ? 'true' : 'false');
  el._prim.classList.toggle('solid', d.status === 'Paused' || (sel && (d.status === 'Downloading' || d.status === 'Queued')));
  el._more.classList.toggle('ring', state.menuFor === d.id);
  const sub = subHtml(d);
  if (el._subHtml !== sub) { el._sub.innerHTML = sub; el._subHtml = sub; }
  const p = pct(d);
  const w = d.status === 'Completed' ? 100 : p ?? 0;
  if (el._fill && el._w !== w) { el._fill.style.width = `${w}%`; el._w = w; }
  const pt = p == null ? '' : `${p}%`;
  if (el._pct && el._p !== pt) { el._pct.textContent = pt; el._p = pt; }
}

function makeRow(d) {
  const el = document.createElement('div');
  el.className = 'row';
  el.dataset.id = d.id;
  el.setAttribute('role', 'option');
  patchRow(el, d);
  return el;
}

function groupEl(key) {
  let g = groupEls.get(key);
  if (!g) {
    g = document.createElement('div');
    g.className = 'group';
    g.textContent = GROUPS[key];
    groupEls.set(key, g);
  }
  return g;
}

/** Ordered list of downloads in the current view, with group keys. */
function layout() {
  const items = visibleDownloads().filter((d) => filterMatch(state.filter, d));
  const queue = items.filter((d) => d.status !== 'Completed');
  const done = items.filter((d) => d.status === 'Completed')
    .sort((a, b) => ((b.finishedAt || 0) - (a.finishedAt || 0)) || (b.id - a.id));
  const seq = [];
  if (queue.length) { seq.push(['g', 'queue']); for (const d of queue) seq.push(['r', d]); }
  for (const key of ['today', 'yesterday', 'earlier']) {
    const ds = done.filter((d) => dayBucket(d.finishedAt) === key);
    if (ds.length) { seq.push(['g', key]); for (const d of ds) seq.push(['r', d]); }
  }
  return { seq, items };
}

export function orderedIds() {
  return layout().seq.filter((x) => x[0] === 'r').map((x) => x[1].id);
}

function renderBar(items, all) {
  const right = els.bar.querySelector('.right');
  els.bar.querySelector('h2').textContent = filterLabel(state.filter);
  if (state.filter === 'completed') {
    const total = items.reduce((s, d) => s + (d.totalSize ?? d.downloaded ?? 0), 0);
    const html = `<span>${items.length} file${items.length === 1 ? '' : 's'} · ${esc(bytes(total))}</span>${items.length ? '<button class="link-btn" data-act="clear">Clear list</button>' : ''}`;
    if (right._html !== html) { right.innerHTML = html; right._html = html; }
    return;
  }
  const active = all.filter((d) => d.status === 'Downloading');
  const sp = active.reduce((s, d) => s + (d.speed || 0), 0);
  const max = (state.settings && state.settings.maxActive) || 3;
  const html = `<span>↓ ${esc(speed(sp))}</span><span>${Math.min(active.length, max)} of ${max} slots</span>`;
  if (right._html !== html) { right.innerHTML = html; right._html = html; }
}

export function renderList() {
  if (!els.list) return;
  const all = visibleDownloads();
  const isEmpty = all.length === 0;
  els.empty.hidden = !isEmpty || !state.loaded;
  els.bar.hidden = isEmpty;
  els.list.hidden = isEmpty;
  const { seq, items } = layout();
  renderBar(items, all);
  if (isEmpty) return;

  const want = [];
  const keep = new Set();
  for (const [kind, v] of seq) {
    if (kind === 'g') { want.push(groupEl(v)); continue; }
    let el = rowEls.get(v.id);
    if (!el) { el = makeRow(v); rowEls.set(v.id, el); } else patchRow(el, v);
    keep.add(v.id);
    want.push(el);
  }
  if (!seq.length) {
    if (!nothingEl) { nothingEl = document.createElement('div'); nothingEl.className = 'nothing'; nothingEl.textContent = 'Nothing here'; }
    want.push(nothingEl);
  }
  for (const [id, el] of rowEls) if (!keep.has(id) && !state.downloads.has(id)) rowEls.delete(id);
  // Reconcile children order without touching nodes that are already in place.
  const list = els.list;
  want.forEach((node, i) => { if (list.children[i] !== node) list.insertBefore(node, list.children[i] || null); });
  while (list.children.length > want.length) list.lastElementChild.remove();
}

function onProgress(changed) {
  for (const d of changed) {
    const el = rowEls.get(d.id);
    if (el && el.isConnected) patchRow(el, d);
  }
  renderBar(layout().items, visibleDownloads());
}

export function select(id, { scroll = true } = {}) {
  const prev = state.selected;
  state.selected = id;
  for (const pid of [prev, id]) {
    const d = state.downloads.get(pid);
    const el = rowEls.get(pid);
    if (d && el) patchRow(el, d);
  }
  if (scroll && id != null) {
    const el = rowEls.get(id);
    if (el) el.scrollIntoView({ block: 'nearest' });
  }
  if (state.drawerId != null && id != null && state.drawerId !== id) openDrawer(id);
}

export function moveSelection(delta) {
  const ids = orderedIds();
  if (!ids.length) return;
  let i = ids.indexOf(state.selected);
  i = i < 0 ? (delta > 0 ? 0 : ids.length - 1) : Math.max(0, Math.min(ids.length - 1, i + delta));
  select(ids[i]);
}

/** Inline rename (F2 / menu). Enter or blur commits, Escape cancels. */
export function startRename(id) {
  const d = state.downloads.get(id);
  const el = rowEls.get(id);
  if (!d || !el || el._renaming) return;
  el.scrollIntoView({ block: 'nearest' });
  const nameEl = el.querySelector('.name');
  el._renaming = true;
  nameEl.innerHTML = '<input class="rename" spellcheck="false" aria-label="File name">';
  const input = nameEl.firstChild;
  input.value = d.fileName;
  input.focus();
  const dot = d.fileName.lastIndexOf('.');
  input.setSelectionRange(0, dot > 0 ? dot : d.fileName.length);
  let done = false;
  const finish = (commit) => {
    if (done) return;
    done = true;
    el._renaming = false;
    const name = input.value.trim();
    el._sig = null;
    patchRow(el, d);
    if (commit && name && name !== d.fileName && !/[\\/]/.test(name)) {
      call('rename', { id, fileName: name }).then((r) => { if (r && r.id != null) { Object.assign(d, r); emit('downloads'); } }, toastError);
    }
  };
  input.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') { e.preventDefault(); finish(true); }
    else if (e.key === 'Escape') { e.preventDefault(); finish(false); }
  });
  input.addEventListener('blur', () => finish(true));
  input.addEventListener('mousedown', (e) => e.stopPropagation());
  input.addEventListener('click', (e) => e.stopPropagation());
}

export function initList(root) {
  els = {
    list: root.querySelector('.list'),
    bar: root.querySelector('.bar'),
    empty: root.querySelector('.empty'),
  };
  els.empty.querySelector('[data-mod]').textContent = `${MOD} V`;
  els.list.setAttribute('role', 'listbox');

  els.list.addEventListener('click', (e) => {
    const rowEl = e.target.closest('.row');
    if (!rowEl) return;
    const d = state.downloads.get(+rowEl.dataset.id);
    if (!d) return;
    const btn = e.target.closest('[data-act]');
    if (btn) {
      e.stopPropagation();
      if (btn.dataset.act === 'primary') A.primary(d);
      else if (btn.dataset.act === 'more') {
        if (btn._skipClick) { btn._skipClick = false; return; }
        select(d.id, { scroll: false });
        A.rowMenu(d, { anchor: btn, keyboard: e.detail === 0 });
      }
      return;
    }
    select(d.id, { scroll: false });
    openDrawer(d.id);
  });
  els.list.addEventListener('dblclick', (e) => {
    const rowEl = e.target.closest('.row');
    if (!rowEl || e.target.closest('[data-act]')) return;
    const d = state.downloads.get(+rowEl.dataset.id);
    if (d && d.status === 'Completed') A.open(d);
  });
  els.list.addEventListener('contextmenu', (e) => {
    const rowEl = e.target.closest('.row');
    if (!rowEl || e.target.closest('input')) return;
    e.preventDefault();
    const d = state.downloads.get(+rowEl.dataset.id);
    if (!d) return;
    select(d.id, { scroll: false });
    A.rowMenu(d, { x: e.clientX, y: e.clientY });
  });
  els.bar.addEventListener('click', (e) => {
    if (e.target.closest('[data-act="clear"]')) {
      A.removeWithUndo(visibleDownloads().filter((d) => d.status === 'Completed'));
    }
  });

  on('downloads', renderList);
  on('filter', () => { els.list.scrollTop = 0; renderList(); });
  on('settings', renderList);
  on('status', renderList);
  on('menu', renderList);
  on('progress', onProgress);
  renderList();
  // Day buckets ("Today" → "Yesterday") move at midnight.
  setInterval(renderList, 60000);
}
