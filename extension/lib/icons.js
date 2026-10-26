// Inline SVG icon sprite (same symbols as the YAD app). No remote resources.

const SYMBOLS = {
  down: '<path d="M12 4v12m0 0-5-5m5 5 5-5M5 20h14"/>',
  video: '<rect x="3" y="5" width="13" height="14" rx="2"/><path d="m16 10 5-3v10l-5-3"/>',
  zip: '<path d="M6 3h9l4 4v14H6z"/><path d="M11 3v2m0 2v2m0 2v2m-1 2h2v3h-2z"/>',
  doc: '<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>',
  app: '<rect x="4" y="4" width="16" height="16" rx="4"/><path d="M9 12h6M12 9v6"/>',
  audio: '<path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>',
  image: '<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="m21 16-5-5-9 9"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1"/>',
  check: '<path d="m5 12 5 5 9-10"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.5 3 14.5 0 18M12 3c-3 3.5-3 14.5 0 18"/>',
  x: '<path d="M6 6l12 12M18 6 6 18"/>',
  retry: '<path d="M4 12a8 8 0 1 0 2.5-5.8M4 4v4h4"/>',
  caret: '<path d="m7 10 5 5 5-5"/>',
  shield: '<path d="M12 3 5 6v6c0 4 3 7.5 7 9 4-1.5 7-5 7-9V6z"/><path d="m9 12 2 2 4-4"/>',
  plug: '<path d="M9 3v5M15 3v5M7 8h10v3a5 5 0 0 1-10 0zM12 16v5"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>',
  trash: '<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
};

/** Insert the sprite once into the current document. */
export function installSprite(doc = document) {
  if (doc.getElementById('yad-sprite')) return;
  const holder = doc.createElement('div');
  holder.innerHTML = `<svg id="yad-sprite" xmlns="http://www.w3.org/2000/svg" style="display:none">${
    Object.entries(SYMBOLS).map(([id, body]) => `<symbol id="${id}" viewBox="0 0 24 24">${body}</symbol>`).join('')}</svg>`;
  doc.body.prepend(holder.firstChild);
}

/** `<svg class="i">` referencing a sprite symbol. */
export function icon(id, attrs = '') {
  return `<svg class="i" aria-hidden="true" ${attrs}><use href="#${id}"/></svg>`;
}

/** File type tile ("video", "zip", "doc", "app", "audio", "image"). */
export function tile(kind) {
  return `<div class="tile t-${kind}">${icon(kind)}</div>`;
}
