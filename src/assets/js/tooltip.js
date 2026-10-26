// Small custom tooltip for elements with data-tip (native title tooltips differ per OS).

let tipEl = null;
let timer = null;
let current = null;

function hide() {
  clearTimeout(timer);
  timer = null;
  current = null;
  if (tipEl) tipEl.hidden = true;
}

function show(el) {
  const text = el.getAttribute('data-tip');
  if (!text || !el.isConnected) return;
  if (!tipEl) {
    tipEl = document.createElement('div');
    tipEl.className = 'tip';
    tipEl.setAttribute('role', 'tooltip');
    document.body.appendChild(tipEl);
  }
  tipEl.textContent = text;
  tipEl.hidden = false;
  const r = el.getBoundingClientRect();
  const t = tipEl.getBoundingClientRect();
  let top = r.bottom + 6;
  if (top + t.height > window.innerHeight - 4) top = r.top - t.height - 6;
  let left = r.left + r.width / 2 - t.width / 2;
  left = Math.max(4, Math.min(left, window.innerWidth - t.width - 4));
  tipEl.style.top = `${Math.round(top)}px`;
  tipEl.style.left = `${Math.round(left)}px`;
}

export function initTooltips() {
  document.addEventListener('mouseover', (e) => {
    const el = e.target.closest && e.target.closest('[data-tip]');
    if (el === current) return;
    hide();
    if (!el) return;
    current = el;
    timer = setTimeout(() => show(el), 450);
  });
  document.addEventListener('mouseleave', hide);
  for (const ev of ['mousedown', 'keydown', 'wheel', 'blur']) window.addEventListener(ev, hide, true);
}
