// Modal sheets: "New download" (ask before download) and a small confirm dialog.
import { call, pickFolder } from './api.js';
import { state, upsert } from './state.js';
import { esc, bytes, crumbs, errText } from './format.js';
import { icon, TYPE_ICON } from './icons.js';
import { openMenu, closeMenu } from './menu.js';
import { toastError, showToast } from './toast.js';

let host = null;
let current = null; // { close }

export const setSheetHost = (el) => { host = el; };
export const isSheetOpen = () => !!current;

function mount(html, { onKey, onClose } = {}) {
  closeSheet();
  closeMenu();
  const scrim = document.createElement('div');
  scrim.className = 'scrim';
  const sheet = document.createElement('div');
  sheet.className = 'sheet';
  sheet.setAttribute('role', 'dialog');
  sheet.setAttribute('aria-modal', 'true');
  sheet.innerHTML = html;
  const prevFocus = document.activeElement;
  (host || document.body).append(scrim, sheet);
  const key = (e) => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); return; }
    if (e.key === 'Tab') { // keep focus inside the sheet
      const f = [...sheet.querySelectorAll('button:not(:disabled), input')];
      if (!f.length) return;
      const i = f.indexOf(document.activeElement);
      const next = e.shiftKey ? (i <= 0 ? f.length - 1 : i - 1) : (i + 1) % f.length;
      f[next].focus();
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    if (onKey) onKey(e);
  };
  // Register on the next tick so the key event that opened the sheet doesn't reach it.
  setTimeout(() => { if (current === handle) document.addEventListener('keydown', key); }, 0);
  scrim.addEventListener('mousedown', () => close());
  function close() {
    if (current !== handle) return;
    current = null;
    document.removeEventListener('keydown', key);
    scrim.remove();
    sheet.remove();
    if (prevFocus && prevFocus.focus && prevFocus.isConnected) prevFocus.focus();
    if (onClose) onClose();
  }
  const handle = { close, sheet };
  current = handle;
  return handle;
}

export function closeSheet() { if (current) current.close(); }

/** Resolves true when confirmed. */
export function confirmSheet({ title, text, ok = 'OK', danger = false }) {
  return new Promise((resolve) => {
    let result = false;
    const h = mount(`<header><h3>${esc(title)}</h3><p class="lead">${esc(text)}</p></header>
      <footer><span class="spacer"></span><button class="btn ghost" data-a="no">Cancel</button><button class="btn${danger ? ' danger' : ''}" data-a="yes">${esc(ok)}</button></footer>`,
    { onClose: () => resolve(result) });
    h.sheet.addEventListener('click', (e) => {
      const a = e.target.closest('[data-a]');
      if (!a) return;
      result = a.dataset.a === 'yes';
      h.close();
    });
    h.sheet.querySelector('[data-a="yes"]').focus();
  });
}

function defaultDir(fileType) {
  const s = state.settings;
  if (!s) return '';
  const sep = s.downloadDir.includes('\\') && !s.downloadDir.includes('/') ? '\\' : '/';
  return s.sortByType && fileType ? `${s.downloadDir.replace(/[\\/]+$/, '')}${sep}${fileType}` : s.downloadDir;
}

function variantLabel(v) {
  const bits = [v.label];
  if (v.resolution && !v.audioOnly) bits.push(v.resolution);
  if (v.estimatedSize) bits.push(bytes(v.estimatedSize));
  return bits.join(' · ');
}
function pickVariant(variants) {
  if (!variants.length) return null;
  const q = state.settings && state.settings.videoQuality;
  if (q === 'audio') return variants.find((v) => v.audioOnly) || variants[0];
  const video = variants.filter((v) => !v.audioOnly);
  if (q && q !== 'best') {
    const want = `${q}p`;
    const hit = video.find((v) => v.label === want || (v.resolution || '').endsWith(`x${q}`));
    if (hit) return hit;
  }
  return video.sort((a, b) => (b.bandwidth || 0) - (a.bandwidth || 0))[0] || variants[0];
}

/** The "New download" sheet. Probes the link to fill size/type/resumable/variants. */
export function openAddSheet(url, source = 'app') {
  const st = {
    probe: null, error: null, fileName: '', dir: '', dirTouched: false, nameTouched: false,
    variant: null, dontAsk: false, busy: false,
  };
  const h = mount(`<header><h3>New download</h3></header>
    <div class="fields">
      <div class="field"><label class="t">Link</label><div class="input">${icon('link')}<span class="v-url"></span></div></div>
      <div class="field"><label class="t" for="sheet-name">Save as</label><div class="input"><input id="sheet-name" spellcheck="false" autocomplete="off"></div></div>
      <div class="field"><label class="t">Folder</label><button class="input" data-a="dir">${icon('folder')}<span class="v-dir"></span>${icon('caret')}</button></div>
      <div class="field" data-variants hidden><label class="t">Quality</label><button class="input" data-a="variant"><span class="v-var"></span>${icon('caret')}</button></div>
      <div class="facts"></div>
    </div>
    <footer><button class="check" data-a="dontask"><span class="cb">${icon('check')}</span>Don't ask again</button>
      <button class="btn ghost" data-a="queue">Add to queue</button><button class="btn" data-a="now">Download now</button></footer>`,
  { onKey: (e) => { if (e.key === 'Enter' && !e.target.closest('button')) { e.preventDefault(); submit(true); } } });
  const $ = (s) => h.sheet.querySelector(s);
  const nameInput = $('#sheet-name');
  $('.v-url').textContent = url;
  nameInput.addEventListener('input', () => { st.nameTouched = true; st.fileName = nameInput.value; });

  function draw() {
    const p = st.probe;
    if (!st.nameTouched) {
      st.fileName = p ? p.fileName : (url.split(/[?#]/)[0].split('/').filter(Boolean).pop() || '');
      try { st.fileName = decodeURIComponent(st.fileName); } catch { /* keep raw */ }
      nameInput.value = st.fileName;
    }
    if (!st.dirTouched) st.dir = defaultDir(p && p.fileType);
    $('.v-dir').textContent = crumbs(st.dir) || 'Default folder';
    const vf = $('[data-variants]');
    vf.hidden = !(p && p.variants && p.variants.length);
    if (!vf.hidden) $('.v-var').textContent = st.variant ? variantLabel(st.variant) : 'Best available';
    $('.cb').classList.toggle('on', st.dontAsk);
    const facts = [];
    if (st.error) {
      facts.push(`<span class="err">${icon('warn')}${esc(`Couldn't check this link: ${st.error}`)}</span>`);
    } else if (!p) {
      facts.push(`<span>${icon('globe')}Checking link…</span>`);
    } else {
      const size = st.variant && st.variant.estimatedSize ? st.variant.estimatedSize : p.size;
      const what = [size != null ? bytes(size) : 'Size unknown', p.contentType || (p.kind !== 'file' ? p.kind.toUpperCase() : '')].filter(Boolean).join(' · ');
      facts.push(`<span>${icon(TYPE_ICON[p.fileType] || 'file')}${esc(what)}</span>`);
      facts.push(p.resumable
        ? `<span class="ok">${icon('check')}Can pause and resume</span>`
        : `<span class="warn">${icon('warn')}Starts over if interrupted</span>`);
      const conns = p.resumable && p.kind === 'file' ? ((state.settings && state.settings.connectionsPerDownload) || 8) : 1;
      facts.push(`<span>${icon('bolt')}${conns} connection${conns === 1 ? '' : 's'}</span>`);
    }
    $('.facts').innerHTML = facts.join('');
  }

  async function submit(start) {
    if (st.busy) return;
    st.busy = true;
    h.sheet.querySelectorAll('.btn').forEach((b) => { b.disabled = true; });
    const params = { url, source };
    const name = nameInput.value.trim();
    if (name && (!st.probe || name !== st.probe.fileName)) params.fileName = name;
    if (st.dirTouched && st.dir) params.directory = st.dir;
    if (st.variant) params.variant = st.variant.id;
    if (!start) params.start = false;
    try {
      if (st.dontAsk) call('setSettings', { askBeforeDownload: false }).then((s) => { if (s) state.settings = s; }, () => {});
      const d = await call('add', params);
      const already = d && state.downloads.has(d.id);
      if (d) upsert(d);
      if (already) showToast(`${d.fileName} is already in the list`, { duration: 3000 });
      h.close();
    } catch (e) {
      st.busy = false;
      h.sheet.querySelectorAll('.btn').forEach((b) => { b.disabled = false; });
      toastError(e);
    }
  }

  h.sheet.addEventListener('click', async (e) => {
    const a = e.target.closest('[data-a]');
    if (!a) return;
    switch (a.dataset.a) {
      case 'dir': {
        try {
          const dir = await pickFolder(st.dir);
          if (dir) { st.dir = dir; st.dirTouched = true; draw(); }
        } catch (err) { toastError(err); }
        break;
      }
      case 'variant': {
        const vs = st.probe ? st.probe.variants : [];
        openMenu({
          anchor: a, align: 'left', width: a.getBoundingClientRect().width, keyboard: e.detail === 0,
          items: vs.map((v) => ({ label: variantLabel(v), checked: st.variant && st.variant.id === v.id, action: () => { st.variant = v; draw(); } })),
        });
        break;
      }
      case 'dontask': st.dontAsk = !st.dontAsk; draw(); break;
      case 'queue': submit(false); break;
      case 'now': submit(true); break;
      default: break;
    }
  });

  draw();
  nameInput.focus();
  call('probe', { url }).then((p) => {
    if (current !== h) return;
    st.probe = p;
    st.variant = pickVariant(p.variants || []);
    draw();
  }, (e) => {
    if (current !== h) return;
    st.error = errText(e);
    draw();
  });
}
