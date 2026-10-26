// SVG icon sprite (same symbols as the approved mockup) and a helper to reference them.

const SPRITE = `<svg xmlns="http://www.w3.org/2000/svg" class="sprite" aria-hidden="true">
<symbol id="i-down" viewBox="0 0 24 24"><path d="M12 4v12m0 0-5-5m5 5 5-5M5 20h14"/></symbol>
<symbol id="i-video" viewBox="0 0 24 24"><rect x="3" y="5" width="13" height="14" rx="2"/><path d="m16 10 5-3v10l-5-3"/></symbol>
<symbol id="i-zip" viewBox="0 0 24 24"><path d="M6 3h9l4 4v14H6z"/><path d="M11 3v2m0 2v2m0 2v2m-1 2h2v3h-2z"/></symbol>
<symbol id="i-doc" viewBox="0 0 24 24"><path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/></symbol>
<symbol id="i-app" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" rx="4"/><path d="M9 12h6M12 9v6"/></symbol>
<symbol id="i-audio" viewBox="0 0 24 24"><path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/></symbol>
<symbol id="i-image" viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="1.8"/><path d="m4 18 5-5 4 4 3-3 4 4"/></symbol>
<symbol id="i-file" viewBox="0 0 24 24"><path d="M6 3h9l4 4v14H6z"/><path d="M15 3v4h4"/></symbol>
<symbol id="i-pause" viewBox="0 0 24 24"><path d="M9 6v12M15 6v12"/></symbol>
<symbol id="i-play" viewBox="0 0 24 24"><path d="M8 5.5v13l10-6.5z"/></symbol>
<symbol id="i-retry" viewBox="0 0 24 24"><path d="M4 12a8 8 0 1 0 2.5-5.8M4 4v4h4"/></symbol>
<symbol id="i-folder" viewBox="0 0 24 24"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></symbol>
<symbol id="i-more" viewBox="0 0 24 24"><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></symbol>
<symbol id="i-link" viewBox="0 0 24 24"><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/></symbol>
<symbol id="i-gear" viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1"/></symbol>
<symbol id="i-check" viewBox="0 0 24 24"><path d="m5 12 5 5 9-10"/></symbol>
<symbol id="i-globe" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.5 3 14.5 0 18M12 3c-3 3.5-3 14.5 0 18"/></symbol>
<symbol id="i-bolt" viewBox="0 0 24 24"><path d="M13 3 5 14h6l-1 7 8-11h-6z"/></symbol>
<symbol id="i-min" viewBox="0 0 24 24"><path d="M5 12h14"/></symbol>
<symbol id="i-max" viewBox="0 0 24 24"><rect x="5" y="5" width="14" height="14" rx="1.5"/></symbol>
<symbol id="i-x" viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></symbol>
<symbol id="i-copy" viewBox="0 0 24 24"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/></symbol>
<symbol id="i-edit" viewBox="0 0 24 24"><path d="M4 20h4L19 9l-4-4L4 16z"/></symbol>
<symbol id="i-trash" viewBox="0 0 24 24"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></symbol>
<symbol id="i-gauge" viewBox="0 0 24 24"><path d="M4 16a8 8 0 1 1 16 0"/><path d="m12 16 4-5"/></symbol>
<symbol id="i-up" viewBox="0 0 24 24"><path d="M12 19V6m0 0-5 5m5-5 5 5"/></symbol>
<symbol id="i-back" viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6"/></symbol>
<symbol id="i-caret" viewBox="0 0 24 24"><path d="m7 10 5 5 5-5"/></symbol>
<symbol id="i-shield" viewBox="0 0 24 24"><path d="M12 3 5 6v6c0 4 3 7.5 7 9 4-1.5 7-5 7-9V6z"/><path d="m9 12 2 2 4-4"/></symbol>
<symbol id="i-plug" viewBox="0 0 24 24"><path d="M9 3v5M15 3v5M7 8h10v3a5 5 0 0 1-10 0zM12 16v5"/></symbol>
<symbol id="i-info" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/></symbol>
<symbol id="i-paint" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 3v18"/></symbol>
<symbol id="i-warn" viewBox="0 0 24 24"><path d="M12 4 3 20h18z"/><path d="M12 10v4M12 17v.5"/></symbol>
</svg>`;

export function injectSprite() {
  if (document.getElementById('i-down')) return;
  const holder = document.createElement('div');
  holder.innerHTML = SPRITE;
  document.body.prepend(holder.firstChild);
}

/** <svg> referencing a sprite symbol. `extra` is trusted (static) markup only. */
export const icon = (id, extra = '') => `<svg class="i" ${extra}><use href="#i-${id}"/></svg>`;

export const TYPE_ICON = {
  Videos: 'video', Audio: 'audio', Documents: 'doc', Programs: 'app',
  Images: 'image', Compressed: 'zip', Others: 'file',
};
const KNOWN_TYPES = Object.keys(TYPE_ICON);
export const typeClass = (t) => `t-${KNOWN_TYPES.includes(t) ? t : 'Others'}`;
export const tile = (t, cls = 'tile') => `<div class="${cls} ${typeClass(t)}">${icon(TYPE_ICON[t] || 'file')}</div>`;
