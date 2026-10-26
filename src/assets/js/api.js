// The only door to the outside world: Tauri bridge commands + daemon events (see docs/ipc.md).
import { errText } from './format.js';

const T = () => window.__TAURI__;

const connListeners = new Set();
let connected = true;
let retryTimer = null;

/** Subscribe to connection changes: cb(connected:boolean). */
export const onConnection = (cb) => { connListeners.add(cb); return () => connListeners.delete(cb); };
export const isConnected = () => connected;

function setConnected(v) {
  if (v === connected) return;
  connected = v;
  for (const cb of connListeners) { try { cb(v); } catch (e) { console.warn(e); } }
  if (!v) scheduleRetry();
  else { clearTimeout(retryTimer); retryTimer = null; }
}

async function ping() {
  try { await T().core.invoke('ipc', { method: 'ping', params: null }); return true; } catch { return false; }
}

function scheduleRetry() {
  clearTimeout(retryTimer);
  retryTimer = setTimeout(async () => {
    retryTimer = null;
    if (await ping()) setConnected(true); else if (!connected) scheduleRetry();
  }, 2000);
}

/** Call a daemon method. Rejects with an Error carrying a human readable message. */
export async function call(method, params) {
  try {
    return await T().core.invoke('ipc', { method, params: params ?? null });
  } catch (e) {
    // Distinguish "daemon said no" from "daemon unreachable" with a ping.
    if (method !== 'ping' && connected && !(await ping())) setConnected(false);
    throw new Error(errText(e));
  }
}

/** Call another bridge command (open_path, reveal_path, show_main, quit_app). */
export async function bridge(cmd, args) {
  try { return await T().core.invoke(cmd, args || {}); } catch (e) { throw new Error(errText(e)); }
}

export const openPath = (path) => bridge('open_path', { path });
export const revealPath = (path) => bridge('reveal_path', { path });

/** Daemon events: cb(Event). Returns a promise of an unlisten function. */
export function onEvent(cb) {
  return T().event.listen('yad-event', (e) => cb(e.payload));
}

export function listenConnection() {
  return T().event.listen('yad-connection', (e) => {
    const c = !!(e.payload && e.payload.connected);
    if (c) setConnected(true); else setConnected(false);
  });
}

export const win = () => T().window.getCurrentWindow();

export async function pickFolder(defaultPath) {
  const r = await T().dialog.open({ directory: true, multiple: false, defaultPath: defaultPath || undefined });
  return Array.isArray(r) ? r[0] : r;
}
