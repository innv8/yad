// YAD Link content script: "Download this video" button over the main video of a frame,
// and Alt-click tracking for the download take-over bypass.
// Classic script (content scripts cannot be modules); everything lives in a closed shadow root.
(() => {
  if (globalThis.__yadLinkLoaded) return;
  globalThis.__yadLinkLoaded = true;

  const HIDE_AFTER_MS = 4000;
  const MIN_W = 200;
  const MIN_H = 112;

  const state = {
    items: [],          // media the background found for this tab
    settings: null,     // { videoQuality, browser: {...} } cached by the background
    video: null,        // the video the button belongs to
    shown: false,
    hover: false,
    menuOpen: false,
    busy: false,
    flash: null,        // { text, tone } shown briefly after sending
  };
  const drmVideos = new WeakSet();
  let hideTimer = 0;
  let flashTimer = 0;
  let tick = 0;

  const alive = () => { try { return !!chrome.runtime?.id; } catch { return false; } };
  const send = (msg) => (alive() ? chrome.runtime.sendMessage(msg) : Promise.reject(new Error('Extension reloaded')));

  // ------------------------------------------------------------ settings and media

  chrome.storage.local.get('settings').then(({ settings }) => { state.settings = settings || null; update(); }).catch(() => {});
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === 'local' && changes.settings) { state.settings = changes.settings.newValue || null; update(); }
  });

  chrome.runtime.onMessage.addListener((msg) => {
    if (msg?.type === 'media') { state.items = msg.items || []; update(); if (state.menuOpen) renderMenu(); }
  });
  send({ type: 'getMedia' }).then((r) => { state.items = r?.items || []; update(); }).catch(() => {});

  const browserSettings = () => state.settings?.browser || { videoButton: true, excludedSites: [] };
  function excluded() {
    const host = location.hostname.replace(/^www\./, '').toLowerCase();
    return (browserSettings().excludedSites || []).some((s) => {
      s = String(s).toLowerCase().replace(/^\*\./, '').replace(/^www\./, '');
      return s && (host === s || host.endsWith(`.${s}`));
    });
  }

  // ------------------------------------------------------------ Alt-click bypass

  const onAltClick = (e) => { if (e.altKey && alive()) send({ type: 'altClick' }).catch(() => {}); };
  document.addEventListener('mousedown', onAltClick, true);
  document.addEventListener('click', onAltClick, true);

  // ------------------------------------------------------------ DRM

  document.addEventListener('encrypted', (e) => { if (e.target instanceof HTMLMediaElement) { drmVideos.add(e.target); update(); } }, true);
  const isProtected = (v) => drmVideos.has(v) || !!v.mediaKeys;

  // ------------------------------------------------------------ picking the video

  const isHttp = (u) => /^https?:/i.test(u || '');

  function visibleRect(v) {
    const r = v.getBoundingClientRect();
    const vw = document.documentElement.clientWidth || innerWidth;
    const vh = document.documentElement.clientHeight || innerHeight;
    const w = Math.min(r.right, vw) - Math.max(r.left, 0);
    const h = Math.min(r.bottom, vh) - Math.max(r.top, 0);
    return { r, w, h, vw, vh };
  }

  function pickVideo() {
    let best = null;
    let bestScore = 0;
    for (const v of document.getElementsByTagName('video')) {
      if (!v.isConnected) continue;
      const { r, w, h } = visibleRect(v);
      if (r.width < MIN_W || r.height < MIN_H || w < 80 || h < 60) continue;
      const playing = !v.paused && !v.ended;
      if (!playing && !v.currentSrc) continue;
      const cs = getComputedStyle(v);
      if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) continue;
      const score = w * h * (playing ? 2 : 1);
      if (score > bestScore) { bestScore = score; best = v; }
    }
    return best;
  }

  /** What the button downloads for the current video: a media item (from the background). */
  function target() {
    const v = state.video;
    if (!v) return null;
    const src = (v.currentSrc || '').split('#')[0];
    if (isHttp(src)) {
      const known = state.items.find((i) => i.url === src);
      if (known) return known;
      const ext = (/\.([a-z0-9]{2,5})(?:$|\?)/i.exec(src) || [])[1] || '';
      return { url: src, kind: 'file', label: ext.toUpperCase() || 'Video', container: ext.toUpperCase(), size: null, variants: [], preferred: null, fileName: '' };
    }
    return state.items.find((i) => i.kind !== 'file') || state.items[0] || null;
  }

  function eligible() {
    const b = browserSettings();
    if (b.videoButton === false || excluded() || !state.video) return false;
    const fs = document.fullscreenElement;
    if (fs && (fs === state.video || fs.tagName === 'VIDEO')) return false;
    return !!target() || isProtected(state.video);
  }

  // ------------------------------------------------------------ shadow UI

  let host = null;
  let root = null;
  let ui = null;

  const ICONS = {
    down: '<path d="M12 4v12m0 0-5-5m5 5 5-5M5 20h14"/>',
    caret: '<path d="m7 10 5 5 5-5"/>',
    check: '<path d="m5 12 5 5 9-10"/>',
    shield: '<path d="M12 3 5 6v6c0 4 3 7.5 7 9 4-1.5 7-5 7-9V6z"/><path d="m9 12 2 2 4-4"/>',
    info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>',
    plug: '<path d="M9 3v5M15 3v5M7 8h10v3a5 5 0 0 1-10 0zM12 16v5"/>',
  };
  const svg = (id, size = 16, extra = '') => `<svg viewBox="0 0 24 24" width="${size}" height="${size}" aria-hidden="true" ${extra}>${ICONS[id]}</svg>`;

  const CSS = `
:host { all: initial; }
.wrap {
  --surface: #f5f5f3; --line: #d8d9d5; --line-strong: #c6c8c3; --fg: #2b2e33; --muted: #676c74; --faint: #959aa0;
  --accent: #4c6a92; --accent-soft: #dce2ea; --bg: #edeeec; --ok: #4f7d62; --warn: #93733f; --err: #a35752;
  --head: 'YAD Link Archivo', 'Archivo Narrow', 'Arial Narrow', sans-serif;
  --text: 'YAD Link Tenor', 'Tenor Sans', 'Gill Sans', sans-serif;
  position: fixed; top: 0; right: 0; display: flex; flex-direction: column; align-items: flex-end; gap: 6px;
  pointer-events: auto; color: var(--fg); font: 400 12.5px/1.25 var(--text); -webkit-font-smoothing: antialiased;
  font-synthesis: none; text-align: left; direction: ltr; letter-spacing: normal; text-transform: none;
  opacity: 0; transform: translateY(-2px); transition: opacity .14s ease, transform .14s ease; visibility: hidden;
}
.wrap.on { opacity: 1; transform: none; visibility: visible; }
@media (prefers-color-scheme: dark) {
  .wrap { --surface: #25282c; --line: #2f3237; --line-strong: #3b3f45; --fg: #cdd0d4; --muted: #8c9199; --faint: #636870;
    --accent: #7f9bc0; --accent-soft: #2c3542; --bg: #1e2023; --ok: #80a78d; --warn: #bea073; --err: #c4877f; }
}
* { box-sizing: border-box; }
svg { stroke: currentColor; fill: none; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; flex: none; display: block; }
button { all: unset; box-sizing: border-box; cursor: pointer; }
button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
[hidden] { display: none !important; }
.pill { display: inline-flex; align-items: stretch; height: 32px; border-radius: 9px; background: var(--surface);
  border: 1px solid var(--line-strong); color: var(--fg); font: 700 12.5px var(--head); overflow: hidden; }
.main { display: inline-flex; align-items: center; gap: 8px; padding: 0 10px 0 7px; white-space: nowrap; max-width: 360px; overflow: hidden; text-overflow: ellipsis; }
.main:hover, .caret:hover, .caret[aria-expanded="true"] { background: var(--accent-soft); }
.main[aria-disabled="true"] { cursor: default; color: var(--muted); }
.main[aria-disabled="true"]:hover { background: transparent; }
.logo { width: 20px; height: 20px; border-radius: 6px; background: var(--fg); color: var(--bg); display: grid; place-items: center; }
.logo svg { stroke-width: 2.4; }
.main.ok .logo { background: var(--ok); }
.main.warn .logo, .main.prot .logo { background: var(--warn); }
.main.err .logo { background: var(--err); }
.caret { display: grid; place-items: center; width: 27px; border-left: 1px solid var(--line); color: var(--muted); }
.spin { width: 11px; height: 11px; border-radius: 50%; border: 2px solid currentColor; border-right-color: transparent; animation: s .8s linear infinite; }
@keyframes s { to { transform: rotate(360deg); } }
.menu { width: 290px; max-height: min(360px, calc(100vh - 70px)); overflow: auto; background: var(--surface);
  border: 1px solid var(--line-strong); border-radius: 10px; padding: 5px; color: var(--fg); }
.menu[hidden] { display: none; }
.qh { padding: 6px 10px 4px; font: 700 10.5px var(--head); letter-spacing: .08em; text-transform: uppercase; color: var(--faint);
  display: flex; justify-content: space-between; gap: 10px; }
.q { display: grid; grid-template-columns: 54px 1fr auto; gap: 8px; align-items: center; padding: 7px 10px; border-radius: 6px;
  font-size: 12.5px; width: 100%; }
.q:hover, .q:focus-visible, .q.hl { background: var(--accent-soft); }
.q:focus-visible { outline: none; }
.q b { font: 700 12.5px var(--head); }
.q span { color: var(--muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.q em { font-style: normal; color: var(--muted); font-size: 12px; white-space: nowrap; }
hr { border: 0; border-top: 1px solid var(--line); margin: 4px 2px; }
.note { padding: 6px 10px 8px; color: var(--muted); font-size: 11.5px; }
@media (prefers-reduced-motion: reduce) { .wrap { transition: none; } .spin { animation: none; } }
`;

  function ensureUi() {
    if (host) return;
    host = document.createElement('yad-link');
    host.setAttribute('style', 'all:initial!important;position:fixed!important;top:0!important;left:0!important;width:0!important;height:0!important;'
      + 'overflow:visible!important;z-index:2147483647!important;display:block!important;pointer-events:none!important;contain:none!important;');
    root = host.attachShadow({ mode: 'closed' });
    root.innerHTML = `<style>${CSS}</style>
      <div class="wrap" part="wrap">
        <div class="pill">
          <button class="main" type="button"></button>
          <button class="caret" type="button" aria-haspopup="menu" aria-expanded="false" aria-label="Choose quality">${svg('caret', 14)}</button>
        </div>
        <div class="menu" role="menu" hidden></div>
      </div>`;
    ui = {
      wrap: root.querySelector('.wrap'),
      main: root.querySelector('.main'),
      caret: root.querySelector('.caret'),
      menu: root.querySelector('.menu'),
    };
    ui.main.addEventListener('click', (e) => { e.stopPropagation(); if (ui.main.getAttribute('aria-disabled') !== 'true') download(null); });
    ui.caret.addEventListener('click', (e) => { e.stopPropagation(); toggleMenu(); });
    ui.menu.addEventListener('click', (e) => {
      e.stopPropagation();
      const row = e.target.closest('.q');
      if (row) { closeMenu(); download(row.dataset.variant || ''); }
    });
    ui.menu.addEventListener('keydown', (e) => {
      const rows = [...ui.menu.querySelectorAll('.q')];
      const i = rows.indexOf(root.activeElement);
      if (e.key === 'ArrowDown') { e.preventDefault(); rows[(i + 1) % rows.length]?.focus(); }
      if (e.key === 'ArrowUp') { e.preventDefault(); rows[(i - 1 + rows.length) % rows.length]?.focus(); }
    });
    // Keep page handlers (players toggle play on click) away from our clicks.
    for (const t of ['click', 'mousedown', 'mouseup', 'pointerdown', 'pointerup', 'dblclick', 'contextmenu']) {
      ui.wrap.addEventListener(t, (e) => e.stopPropagation());
    }
    ui.wrap.addEventListener('pointerenter', () => { state.hover = true; clearTimeout(hideTimer); });
    ui.wrap.addEventListener('pointerleave', () => { state.hover = false; scheduleHide(); });
    loadFonts();
    attachHost();
  }

  function attachHost() {
    const fs = document.fullscreenElement;
    const parent = fs && fs.tagName !== 'VIDEO' ? fs : document.documentElement;
    if (host.parentNode !== parent) parent.appendChild(host);
  }

  let fontsLoaded = false;
  async function loadFonts() {
    if (fontsLoaded || !alive()) return;
    fontsLoaded = true;
    // FontFace from bytes: works inside shadow roots and is not subject to the page's font-src CSP.
    const load = async (family, file, descriptors) => {
      const buf = await (await fetch(chrome.runtime.getURL(`fonts/${file}`))).arrayBuffer();
      const face = new FontFace(family, buf, descriptors);
      await face.load();
      document.fonts.add(face);
    };
    try {
      await Promise.all([
        load('YAD Link Archivo', 'archivo-narrow-latin.woff2', { weight: '400 700' }),
        load('YAD Link Tenor', 'tenor-sans-latin.woff2', { weight: '400' }),
      ]);
    } catch { /* fall back to system fonts */ }
  }

  // ------------------------------------------------------------ rendering

  function renderButton() {
    const m = ui.main;
    m.className = 'main';
    m.removeAttribute('aria-disabled');
    m.title = '';
    ui.caret.hidden = false;
    if (state.video && isProtected(state.video)) {
      m.classList.add('prot');
      m.setAttribute('aria-disabled', 'true');
      m.title = 'This video is protected (DRM) and cannot be downloaded';
      m.innerHTML = `<span class="logo">${svg('shield', 12)}</span>Protected video`;
      ui.caret.hidden = true;
      return;
    }
    if (state.flash) {
      m.classList.add(state.flash.tone);
      m.title = state.flash.title || '';
      m.innerHTML = `<span class="logo">${svg(state.flash.icon, 12)}</span>${esc(state.flash.text)}`;
      return;
    }
    if (state.busy) { m.innerHTML = `<span class="logo"><span class="spin"></span></span>Sending to YAD…`; return; }
    m.innerHTML = `<span class="logo">${svg('down', 12)}</span>Download this video`;
  }

  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  function size(bytes) {
    if (bytes == null || !(bytes >= 0)) return '';
    if (bytes < 1024) return `${bytes} B`;
    const u = ['KB', 'MB', 'GB', 'TB'];
    let v = bytes / 1024; let i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return `${v < 10 ? v.toFixed(1).replace(/\.0$/, '') : Math.round(v)} ${u[i]}`;
  }

  function renderMenu() {
    const t = target();
    if (!t) { ui.menu.innerHTML = '<div class="note">No downloadable video found yet.</div>'; return; }
    const row = (variant, label, detail, bytes, hl) => `<button class="q${hl ? ' hl' : ''}" role="menuitem" data-variant="${esc(variant)}">`
      + `<b>${esc(label)}</b><span>${esc(detail)}</span><em>${esc(size(bytes))}</em></button>`;
    let html;
    if (t.error) {
      html = `<div class="qh"><span>Can't download</span><span>${esc(t.label)}</span></div><div class="note">${esc(t.error)}</div>`;
    } else if (t.variants?.length) {
      const video = t.variants.filter((v) => !v.audioOnly);
      const audio = t.variants.filter((v) => v.audioOnly);
      const n = t.variants.length;
      html = `<div class="qh"><span>Found ${n} stream${n === 1 ? '' : 's'}</span><span>${esc(t.label)}</span></div>`
        + video.map((v) => row(v.id, v.label, v.detail, v.size, v.id === t.preferred)).join('')
        + (video.length && audio.length ? '<hr>' : '')
        + audio.map((v) => row(v.id, v.label, v.detail, v.size, v.id === t.preferred)).join('');
    } else if (t.kind !== 'file') {
      html = `<div class="qh"><span>${t.probing ? 'Finding streams…' : 'Found 1 stream'}</span><span>${esc(t.label)}</span></div>`
        + row('', 'Best', 'Highest quality available', t.size, true);
    } else {
      const v = state.video;
      const label = v && v.videoHeight ? `${v.videoHeight}p` : (t.audio ? 'Audio' : 'Original');
      html = `<div class="qh"><span>Found 1 file</span><span>${esc(t.container || t.label)}</span></div>`
        + row('', label, [t.container, t.fileName].filter(Boolean).join(' · '), t.size, true);
    }
    ui.menu.innerHTML = html;
  }

  function position() {
    if (!state.shown || !state.video) return;
    const { r, vw, vh } = visibleRect(state.video);
    const top = Math.max(r.top, 0) + 12;
    const right = vw - Math.min(r.right, vw) + 12;
    if (r.bottom - top < 40 || r.right < 60 || r.top > vh) { setShown(false); return; }
    ui.wrap.style.top = `${Math.round(top)}px`;
    ui.wrap.style.right = `${Math.round(right)}px`;
    ui.wrap.style.left = 'auto';
  }

  function setShown(on) {
    if (on && !eligible()) on = false;
    if (on) {
      ensureUi();
      attachHost();
      renderButton();
      if (!state.shown) { state.shown = true; position(); requestAnimationFrame(() => ui.wrap.classList.add('on')); }
      position();
      if (!tick) tick = setInterval(() => { if (state.shown) { if (!state.video?.isConnected) update(); else position(); } }, 400);
    } else {
      if (state.menuOpen) closeMenu();
      state.shown = false;
      if (ui) ui.wrap.classList.remove('on');
      clearInterval(tick); tick = 0;
    }
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => { if (!state.hover && !state.menuOpen && !state.busy) setShown(false); }, HIDE_AFTER_MS);
  }

  /** Show the button now and hide it again a little later unless the pointer stays. */
  function reveal() {
    setShown(true);
    if (!state.hover) scheduleHide(); else clearTimeout(hideTimer);
  }

  function update() {
    const v = pickVideo() || (state.video?.isConnected ? state.video : null);
    if (v !== state.video) { state.video = v; if (state.menuOpen) closeMenu(); }
    if (!eligible()) { setShown(false); return; }
    if (state.shown) { renderButton(); position(); }
  }

  // ------------------------------------------------------------ menu

  function toggleMenu() { if (state.menuOpen) closeMenu(); else openMenu(); }
  function openMenu() {
    renderMenu();
    ui.menu.hidden = false;
    ui.caret.setAttribute('aria-expanded', 'true');
    state.menuOpen = true;
    clearTimeout(hideTimer);
    (ui.menu.querySelector('.q.hl') || ui.menu.querySelector('.q'))?.focus({ preventScroll: true });
  }
  function closeMenu() {
    if (!ui) return;
    ui.menu.hidden = true;
    ui.caret.setAttribute('aria-expanded', 'false');
    state.menuOpen = false;
    if (!state.hover) scheduleHide();
  }

  // ------------------------------------------------------------ sending

  async function download(variant) {
    const t = target();
    if (!t || state.busy) return;
    if (variant === null) variant = t.preferred || '';
    state.busy = true;
    renderButton();
    let res;
    try {
      res = await send({
        type: 'add', url: t.url, variant: variant || undefined,
        pageUrl: window === top ? location.href : undefined, referrer: location.href,
        title: window === top ? document.title : undefined,
      });
    } catch (err) {
      res = { ok: false, error: err.message, offline: true };
    }
    state.busy = false;
    if (res?.ok) flash({ text: 'Sent to YAD', tone: 'ok', icon: 'check' });
    else if (res?.offline) flash({ text: "YAD isn't running", tone: 'warn', icon: 'plug', title: res.error || '' });
    else flash({ text: res?.error || "Couldn't send to YAD", tone: 'err', icon: 'info', title: res?.error || '', long: true });
  }

  function flash(f) {
    clearTimeout(flashTimer);
    state.flash = f;
    renderButton();
    clearTimeout(hideTimer);
    flashTimer = setTimeout(() => { state.flash = null; if (ui) renderButton(); if (!state.hover) scheduleHide(); }, f.long ? 5000 : 2600);
  }

  // ------------------------------------------------------------ page events

  const mediaEvents = ['play', 'playing', 'loadedmetadata', 'emptied', 'pause', 'ended'];
  for (const t of mediaEvents) {
    document.addEventListener(t, (e) => {
      if (!(e.target instanceof HTMLVideoElement)) return;
      update();
      if ((t === 'play' || t === 'playing') && e.target === state.video) reveal();
    }, true);
  }

  let moveFrame = 0;
  document.addEventListener('pointermove', (e) => {
    if (moveFrame) return;
    const { clientX: x, clientY: y } = e;
    const path0 = e.target;
    moveFrame = requestAnimationFrame(() => {
      moveFrame = 0;
      if (!state.video) { update(); if (!state.video) return; }
      const r = state.video.getBoundingClientRect();
      const over = path0 === host || (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom);
      if (over && !state.hover) { state.hover = true; clearTimeout(hideTimer); setShown(true); } else if (over) { if (!state.shown) setShown(true); } else if (state.hover) { state.hover = false; scheduleHide(); }
    });
  }, { capture: true, passive: true });

  document.addEventListener('pointerdown', (e) => { if (state.menuOpen && e.target !== host) closeMenu(); }, true);
  document.addEventListener('keydown', (e) => { if (state.menuOpen && e.key === 'Escape') { closeMenu(); ui.caret.focus(); } }, true);
  addEventListener('scroll', () => { if (state.shown) position(); }, { capture: true, passive: true });
  addEventListener('resize', () => { if (state.shown) position(); }, { passive: true });
  document.addEventListener('fullscreenchange', () => {
    if (host) attachHost();
    update();
    if (state.shown) position();
  });

  // Videos that were already playing before we loaded.
  update();
  if (state.video && !state.video.paused) reveal();
})();
