// Custom popover menu used for the row context menu and for dropdowns (no native <select>).
import { icon } from './icons.js';
import { esc } from './format.js';

let open = null; // { el, items, hl, onClose, cleanup }

export const isMenuOpen = () => !!open;

export function closeMenu() {
  if (!open) return;
  const o = open;
  open = null;
  o.cleanup();
  o.el.remove();
  if (o.onClose) o.onClose();
}

/**
 * openMenu({ items, anchor?, x?, y?, width?, onClose? })
 * items: [{ label, icon?, kbd?, danger?, checked?, action }] or 'sep'
 * Labels are escaped. Positioned below/right-aligned to `anchor`, or at (x, y).
 */
export function openMenu({ items, anchor, x, y, width, align = 'right', onClose, keyboard = false }) {
  closeMenu();
  const el = document.createElement('div');
  el.className = 'menu';
  el.setAttribute('role', 'menu');
  if (width) el.style.width = `${width}px`;
  const acts = [];
  el.innerHTML = items.map((it) => {
    if (it === 'sep') return '<hr>';
    const i = acts.push(it) - 1;
    return `<button class="mi${it.danger ? ' danger' : ''}" role="menuitem" data-i="${i}">${it.icon ? icon(it.icon) : ''}<span>${esc(it.label)}</span>${it.kbd ? `<kbd>${esc(it.kbd)}</kbd>` : ''}${it.checked ? `<span class="tick">${icon('check')}</span>` : ''}</button>`;
  }).join('');
  document.body.appendChild(el);

  const mr = el.getBoundingClientRect();
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  let left;
  let top;
  if (anchor) {
    const r = anchor.getBoundingClientRect();
    left = align === 'left' ? r.left : r.right - mr.width;
    top = r.bottom + 4;
    if (top + mr.height > vh - 8) top = Math.max(8, r.top - mr.height - 4);
  } else {
    left = x;
    top = y;
    if (top + mr.height > vh - 8) top = Math.max(8, y - mr.height);
  }
  left = Math.max(8, Math.min(left, vw - mr.width - 8));
  top = Math.max(8, Math.min(top, vh - mr.height - 8));
  el.style.left = `${Math.round(left)}px`;
  el.style.top = `${Math.round(top)}px`;

  const btns = [...el.querySelectorAll('.mi')];
  const setHl = (i) => {
    btns.forEach((b, j) => b.classList.toggle('hl', j === i));
    if (open) open.hl = i;
  };
  const run = (i) => {
    const it = acts[i];
    closeMenu();
    if (it && it.action) it.action();
  };
  el.addEventListener('mousemove', (e) => {
    const b = e.target.closest('.mi');
    if (b) setHl(+b.dataset.i);
  });
  el.addEventListener('mouseleave', () => setHl(-1));
  el.addEventListener('click', (e) => {
    const b = e.target.closest('.mi');
    if (b) run(+b.dataset.i);
  });
  el.addEventListener('contextmenu', (e) => e.preventDefault());

  const onKey = (e) => {
    if (!open) return;
    const n = btns.length;
    if (e.key === 'ArrowDown') { setHl((open.hl + 1 + n) % n); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { setHl(open.hl <= 0 ? n - 1 : open.hl - 1); e.preventDefault(); }
    else if (e.key === 'Enter' || e.key === ' ') { if (open.hl >= 0) run(open.hl); else closeMenu(); e.preventDefault(); }
    else if (e.key === 'Escape' || e.key === 'Tab') { closeMenu(); e.preventDefault(); }
    else return;
    e.stopPropagation();
  };
  const onDown = (e) => { if (!el.contains(e.target) && !(anchor && anchor.contains(e.target))) closeMenu(); };
  const onAnchorDown = (e) => { if (anchor && anchor.contains(e.target)) { anchor._skipClick = true; setTimeout(() => { anchor._skipClick = false; }, 600); closeMenu(); } };
  const onBlur = () => closeMenu();
  document.addEventListener('keydown', onKey, true);
  document.addEventListener('mousedown', onDown, true);
  window.addEventListener('blur', onBlur);
  window.addEventListener('resize', onBlur);
  // A second press on the anchor closes the menu instead of reopening it.
  if (anchor) anchor.addEventListener('mousedown', onAnchorDown, { once: true });

  open = {
    el, hl: -1, onClose,
    cleanup() {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('resize', onBlur);
      if (anchor) anchor.removeEventListener('mousedown', onAnchorDown);
    },
  };
  // Keyboard-opened menus start with the first item highlighted.
  if (keyboard) setHl(0);
  return el;
}

/** Open a menu from a button click; a second click on the same button just closes it. */
export function menuFromButton(anchor, event, opts) {
  if (anchor._skipClick) { anchor._skipClick = false; return null; }
  return openMenu({ ...opts, anchor, keyboard: event ? event.detail === 0 : false });
}
