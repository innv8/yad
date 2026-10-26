// Custom 34px title bar (the window has decorations:false).
// Dragging and double-click-to-maximize come from Tauri's own drag-region script
// (tauri >= 2.x supports data-tauri-drag-region="deep"; buttons inside are excluded automatically).
// We deliberately don't add a dblclick handler: it would toggle maximize twice.
import { win } from './api.js';
import { icon } from './icons.js';

export function initTitlebar(bar) {
  bar.setAttribute('data-tauri-drag-region', 'deep');
  bar.innerHTML = `<div class="brand"><div class="logo">${icon('down')}</div><span>YAD</span></div>
    <div class="ttl"></div>
    <div class="wc">
      <button data-w="min" data-tip="Minimize" aria-label="Minimize">${icon('min')}</button>
      <button data-w="max" data-tip="Maximize" aria-label="Maximize">${icon('max')}</button>
      <button data-w="close" class="close" data-tip="Close" aria-label="Close">${icon('x')}</button>
    </div>`;
  bar.querySelector('.wc').addEventListener('click', (e) => {
    const b = e.target.closest('[data-w]');
    if (!b) return;
    const w = win();
    // close(): the Rust side decides whether to hide (keepRunning) or quit.
    ({ min: () => w.minimize(), max: () => w.toggleMaximize(), close: () => w.close() })[b.dataset.w]();
  });
}

export function setTitle(bar, text) {
  bar.querySelector('.ttl').textContent = text || '';
}
