// One toast at a time, anchored to the bottom of the download list.
// A new toast finishes the previous one first (its onExpire runs, e.g. a deferred remove).

let host = null;
let active = null; // { el, timer, onExpire }

export function setToastHost(el) { host = el; }

function finish(runExpire) {
  if (!active) return;
  const a = active;
  active = null;
  clearTimeout(a.timer);
  a.el.remove();
  if (runExpire && a.onExpire) a.onExpire();
}

/**
 * showToast("Removed x from the list", { action: 'Undo', onAction, onExpire, duration })
 * onExpire runs when the toast times out or is replaced; onAction runs instead when clicked.
 */
export function showToast(text, opts = {}) {
  finish(true);
  const el = document.createElement('div');
  el.className = 'toast';
  el.setAttribute('role', 'status');
  const span = document.createElement('span');
  span.textContent = text;
  el.appendChild(span);
  if (opts.action) {
    const b = document.createElement('button');
    b.textContent = opts.action;
    b.addEventListener('click', () => {
      const cb = opts.onAction;
      finish(false);
      if (cb) cb();
    });
    el.appendChild(b);
  }
  (host || document.body).appendChild(el);
  active = { el, onExpire: opts.onExpire, timer: setTimeout(() => finish(true), opts.duration || 6000) };
}

export const toastError = (e) => showToast(e && e.message ? e.message : String(e), { duration: 4000 });

/** Run the pending toast's onExpire now (e.g. before the window goes away). */
export const flushToast = () => finish(true);
