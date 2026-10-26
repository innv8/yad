// Details drawer: progress, per-segment bars, 60 s speed sparkline, key/value facts.
// Polls `get` once a second while open; progress events patch the header in between.
import { state, on, emit } from './state.js';
import { call } from './api.js';
import { esc, bytes, speed, pct, etaLong, whenLong, stripScheme, tildify } from './format.js';
import { icon } from './icons.js';
import * as A from './actions.js';

let el = null;
let wrap = null;
let timer = null;
let details = null; // last DownloadDetails for drawerId
let shownId = null;

export function initDetails(drawerEl, wrapEl) {
  el = drawerEl;
  wrap = wrapEl;
  el.addEventListener('click', (e) => {
    const b = e.target.closest('[data-a]');
    if (!b) return;
    const d = state.downloads.get(state.drawerId);
    if (b.dataset.a === 'close') return closeDrawer();
    if (!d) return;
    ({
      pause: () => A.pause(d), resume: () => A.resume(d), retry: () => A.retry(d), cancel: () => A.cancel(d),
      open: () => A.open(d), reveal: () => A.reveal(d), remove: () => A.removeWithUndo([d]),
    })[b.dataset.a]?.();
  });
  on('drawer', sync);
  on('downloads', () => { if (state.drawerId != null) draw(); });
  on('progress', (ds) => { if (ds.some((d) => d.id === state.drawerId)) drawHead(); });
}

export function openDrawer(id) {
  if (state.drawerId === id && shownId === id) return;
  state.drawerId = id;
  sync();
}
export function closeDrawer() {
  if (state.drawerId == null) return;
  state.drawerId = null;
  sync();
}

function sync() {
  const id = state.drawerId;
  wrap.classList.toggle('has-drawer', id != null);
  el.hidden = id == null;
  clearInterval(timer);
  timer = null;
  if (id == null) { shownId = null; details = null; el.innerHTML = ''; return; }
  if (shownId !== id) {
    shownId = id;
    details = null;
    el.innerHTML = `<div class="dh"><h3 class="d-name"></h3><button class="icon-btn" data-a="close" data-tip="Close" aria-label="Close">${icon('x')}</button></div>
      <div><div class="prog"><span class="d-of"></span><span class="d-pct"></span></div><div class="track big"><div class="fill d-fill"></div></div><div class="acts d-acts"></div></div>
      <div class="d-segs-wrap"><h4 class="d-conns"></h4><div class="segs d-segs"></div></div>
      <div><h4>Speed · last 60 s</h4><svg class="spark d-spark" viewBox="0 0 250 54" preserveAspectRatio="none"></svg><div class="spark-legend"><span class="d-avg"></span><span class="d-now"></span></div></div>
      <dl class="kv d-kv"></dl>`;
    el._acts = null;
  }
  draw();
  poll();
  timer = setInterval(poll, 1000);
}

async function poll() {
  const id = state.drawerId;
  if (id == null) return;
  try {
    const r = await call('get', { id });
    if (state.drawerId !== id || !r) return;
    details = r;
    const d = state.downloads.get(id);
    if (d) {
      // `get` is fresher than the list copy; keep the row in sync without a re-render storm.
      const statusChanged = d.status !== r.status || d.fileName !== r.fileName;
      for (const k of ['downloaded', 'totalSize', 'speed', 'eta', 'connections', 'status', 'fileName', 'error', 'retries', 'finishedAt', 'destinationPath']) d[k] = r[k];
      if (statusChanged) emit('downloads'); else emit('progress', [d]);
    }
    draw();
  } catch { /* banner handles connectivity; keep last data */ }
}

const q = (s) => el.querySelector(s);
const setText = (s, t) => { const n = q(s); if (n && n.textContent !== t) n.textContent = t; };

function drawHead() {
  const d = state.downloads.get(state.drawerId);
  if (!d || !el.firstChild) return;
  setText('.d-name', d.fileName);
  const p = pct(d);
  setText('.d-of', d.totalSize ? `${bytes(d.downloaded)} of ${bytes(d.totalSize)}` : bytes(d.downloaded));
  setText('.d-pct', d.status === 'Completed' ? '100%' : p == null ? '' : `${p}%`);
  const fill = q('.d-fill');
  fill.style.width = `${d.status === 'Completed' ? 100 : p ?? 0}%`;
  fill.className = `fill d-fill ${({ Paused: 'paused', Failed: 'err', Cancelled: 'err', Queued: 'idle' })[d.status] || ''}`;
  const acts = {
    Downloading: [['pause', 'Pause', 'pause', ''], ['cancel', 'Cancel', '', 'ghost']],
    Queued: [['pause', 'Pause', 'pause', ''], ['cancel', 'Cancel', '', 'ghost']],
    Paused: [['resume', 'Resume', 'play', ''], ['cancel', 'Cancel', '', 'ghost']],
    Failed: [['retry', 'Retry', 'retry', ''], ['remove', 'Remove', '', 'ghost']],
    Cancelled: [['retry', 'Retry', 'retry', ''], ['remove', 'Remove', '', 'ghost']],
    Completed: [['open', 'Open', 'doc', ''], ['reveal', 'Show in folder', '', 'ghost']],
  }[d.status] || [];
  const html = acts.map(([a, label, ic, cls]) => `<button class="btn ${cls}" data-a="${a}">${ic ? icon(ic) : ''}${label}</button>`).join('');
  const box = q('.d-acts');
  if (el._acts !== html) { box.innerHTML = html; el._acts = html; }
}

function sparkPoints(hist) {
  const n = hist.length;
  if (n < 2) return null;
  const max = Math.max(...hist, 1);
  return hist.map((v, i) => `${((i / (n - 1)) * 250).toFixed(1)},${(52 - (v / max) * 44).toFixed(1)}`).join(' ');
}

function draw() {
  const d = state.downloads.get(state.drawerId);
  if (!d) return;
  drawHead();
  const det = details && details.id === d.id ? details : null;

  // segments
  const segs = det ? det.segments || [] : [];
  const conns = d.status === 'Downloading' ? d.connections : 0;
  setText('.d-conns', conns ? `${conns} connection${conns === 1 ? '' : 's'}` : `${segs.length || 1} part${segs.length === 1 || !segs.length ? '' : 's'}`);
  q('.d-segs-wrap').hidden = !segs.length;
  const segBox = q('.d-segs');
  segBox.style.gridTemplateColumns = `repeat(${Math.max(1, Math.min(8, segs.length))}, minmax(0, 1fr))`;
  segBox.classList.toggle('done', d.status === 'Completed');
  const segHtml = segs.map((s) => {
    const len = s.end - s.start + 1;
    const p = len > 0 ? Math.max(0, Math.min(100, (s.downloaded / len) * 100)) : 0;
    return `<div><i data-w="${p.toFixed(1)}"></i></div>`;
  }).join('');
  if (segBox._html !== segHtml) {
    segBox.innerHTML = segHtml;
    segBox._html = segHtml;
    // Widths via CSSOM, not style attributes, so a strict CSP (style-src 'self') is fine.
    for (const i of segBox.querySelectorAll('i')) i.style.width = `${i.dataset.w}%`;
  }

  // sparkline
  const hist = det ? det.speedHistory || [] : [];
  const pts = sparkPoints(hist);
  const spark = q('.d-spark');
  const sh = pts ? `<polyline fill="var(--accent-soft)" stroke="none" points="0,54 ${pts} 250,54"/><polyline fill="none" stroke="var(--accent)" stroke-width="1.6" vector-effect="non-scaling-stroke" points="${pts}"/>`
    : '<line x1="0" y1="52" x2="250" y2="52" stroke="var(--line-strong)" stroke-width="1" vector-effect="non-scaling-stroke"/>';
  if (spark._html !== sh) { spark.innerHTML = sh; spark._html = sh; }
  const nz = hist.filter((v) => v > 0);
  setText('.d-avg', `avg ${speed(nz.length ? nz.reduce((a, b) => a + b, 0) / nz.length : 0)}`);
  setText('.d-now', `now ${speed(d.status === 'Downloading' ? d.speed : 0)}`);

  // facts
  const kv = [['Link', stripScheme(d.url)]];
  if (d.pageUrl) kv.push(['Found on', stripScheme(d.pageUrl)]);
  kv.push(['Saved to', tildify(d.destinationDir).replace(/^~\//, '')]);
  if (d.variant) kv.push(['Quality', d.variant]);
  const res = d.resumable ? (det && det.etag ? 'Yes · ETag checked' : det && det.lastModified ? 'Yes · date checked' : 'Yes') : 'No';
  kv.push(['Resumable', res]);
  kv.push(['Started', whenLong(d.createdAt)]);
  if (d.status === 'Completed') kv.push(['Finished', whenLong(d.finishedAt)]);
  else if (d.status === 'Downloading') kv.push(['Time left', etaLong(d.eta)]);
  if (d.error && d.status !== 'Completed') kv.push(['Error', d.error]);
  if (d.retries) kv.push(['Retries', String(d.retries)]);
  const kvHtml = kv.map(([k, v]) => `<dt>${esc(k)}</dt><dd data-tip="${esc(v)}">${esc(v)}</dd>`).join('');
  const kvEl = q('.d-kv');
  if (kvEl._html !== kvHtml) { kvEl.innerHTML = kvHtml; kvEl._html = kvHtml; }
}
