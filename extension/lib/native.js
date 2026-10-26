// Native messaging client for `yad --native-host` (host name ke.innv8.yad).
// See docs/ipc.md: requests {id, method, params}, responses {id, result} | {id, error:{message}}.
//
// Only the background service worker uses this. Popup and options talk to the worker,
// so there is never more than one host process per browser profile.

export const HOST = 'ke.innv8.yad';
const CALL_TIMEOUT = 10_000;
const HELLO_TIMEOUT = 10_000;
const START_TIMEOUT = 20_000; // `start`, `add` and `probe` may have to start YAD first (~5 s)

/** Methods the host answers by starting YAD when it is down (user actions only). */
export const STARTS_YAD = new Set(['start', 'add', 'probe']);
const BACKOFF = [1_000, 2_000, 5_000, 10_000, 30_000, 60_000, 300_000];

export class NativeError extends Error {
  constructor(message, { offline = false } = {}) {
    super(message);
    this.name = 'NativeError';
    this.offline = offline;
  }
}

/** Turn chrome.runtime.lastError text into something a person can act on. */
export function explainDisconnect(message) {
  const m = String(message || '');
  if (/isn't running|is not running/i.test(m)) return { code: 'offline', text: "YAD isn't running." };
  if (/not found/i.test(m)) return { code: 'not-installed', text: "YAD's browser link isn't installed on this computer." };
  if (/forbidden/i.test(m)) return { code: 'forbidden', text: "YAD doesn't recognise this copy of the extension." };
  if (/exited|crashed/i.test(m)) return { code: 'exited', text: 'YAD stopped before it could answer.' };
  if (/timed out|in time/i.test(m)) return { code: 'timeout', text: 'YAD took too long to answer.' };
  return { code: 'offline', text: m || "YAD isn't running." };
}

export class NativeClient {
  /**
   * Two layers: the port (the host process, `yad --native-host`) and the YAD service behind it.
   * `connected` means `hello` succeeded, i.e. the service is running. The host answers `hello`
   * with "YAD isn't running." when it is down; the port then stays open and `hello` is retried
   * with backoff. `start`, `add` and `probe` only need the port: the host starts YAD for them.
   *
   * @param {{ hello: () => object, onState?: (s) => void, onEvent?: (e) => void }} opts
   */
  constructor({ hello, onState, onEvent } = {}) {
    this.helloParams = hello;
    this.onState = onState || (() => {});
    this.onEvent = onEvent || (() => {});
    this.port = null;
    this.nextId = 1;
    this.pending = new Map();
    this.ready = null;          // Promise<Status> while a hello is in flight
    this.state = { connected: false, connecting: false, status: null, error: null };
    this.attempt = 0;
    this.retryTimer = null;
    this.autoReconnect = false; // once the host has answered at least once
  }

  get connected() { return this.state.connected; }

  setState(patch) {
    this.state = { ...this.state, ...patch };
    try { this.onState(this.state); } catch (e) { console.error(e); }
  }

  /** Open the port if needed (host process). Throws NativeError when it can't be opened. */
  ensurePort() {
    if (this.port) return this.port;
    let port;
    try {
      port = chrome.runtime.connectNative(HOST);
    } catch (e) {
      const why = explainDisconnect(e?.message);
      this.setState({ connecting: false, connected: false, error: why });
      throw new NativeError(why.text, { offline: true });
    }
    this.port = port;
    port.onMessage.addListener((msg) => this.handle(msg));
    port.onDisconnect.addListener(() => this.handleDisconnect(port, chrome.runtime.lastError?.message));
    return port;
  }

  /** Say hello if needed; resolves with Status when YAD is running, rejects with NativeError (offline). */
  connect({ force = false } = {}) {
    if (force) { this.attempt = 0; clearTimeout(this.retryTimer); this.retryTimer = null; }
    if (this.connected && !force) return Promise.resolve(this.state.status);
    if (this.ready) return this.ready;
    this.ready = this.hello().finally(() => { this.ready = null; });
    return this.ready;
  }

  async hello() {
    this.setState({ connecting: true });
    this.ensurePort();
    try {
      const status = await this.send('hello', this.helloParams(), HELLO_TIMEOUT);
      this.attempt = 0;
      this.autoReconnect = true;
      clearTimeout(this.retryTimer); this.retryTimer = null;
      this.setState({ connecting: false, connected: true, status, error: null });
      return status;
    } catch (err) {
      if (this.port && !err.offline) {
        // The host answered: it is installed, YAD itself is down ("YAD isn't running.").
        this.autoReconnect = true;
        const why = explainDisconnect(err.message);
        this.setState({ connecting: false, connected: false, error: why });
        this.scheduleReconnect();
        throw new NativeError(why.text, { offline: true });
      }
      if (this.state.connecting) { // timed out with the port still open
        this.setState({ connecting: false, connected: false, error: explainDisconnect(err.message) });
        if (this.autoReconnect) this.scheduleReconnect();
      }
      throw err instanceof NativeError ? err : new NativeError(err.message, { offline: true });
    }
  }

  handle(msg) {
    if (msg && msg.event) { try { this.onEvent(msg.event); } catch (e) { console.error(e); } return; }
    const p = msg && this.pending.get(msg.id);
    if (!p) return;
    this.pending.delete(msg.id);
    clearTimeout(p.timer);
    if (msg.error) {
      const text = msg.error.message || 'YAD returned an error';
      p.reject(new NativeError(text, { offline: /isn't running/i.test(text) && p.method !== 'hello' }));
    } else p.resolve(msg.result);
  }

  handleDisconnect(port, message) {
    if (this.port !== port) return;
    this.port = null;
    const why = explainDisconnect(message);
    for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(new NativeError(why.text, { offline: true })); }
    this.pending.clear();
    this.setState({ connected: false, connecting: false, error: why });
    if (this.autoReconnect && why.code !== 'not-installed' && why.code !== 'forbidden') this.scheduleReconnect();
  }

  scheduleReconnect() {
    if (this.retryTimer) return;
    const delay = BACKOFF[Math.min(this.attempt, BACKOFF.length - 1)];
    this.attempt++;
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.connect().catch(() => this.scheduleReconnect());
    }, delay);
  }

  send(method, params, timeout = CALL_TIMEOUT) {
    const port = this.port;
    if (!port) return Promise.reject(new NativeError("YAD isn't running.", { offline: true }));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new NativeError(`YAD did not answer "${method}" in time`, { offline: method === 'hello' }));
      }, timeout);
      this.pending.set(id, { resolve, reject, timer, method });
      try {
        port.postMessage(params === undefined ? { id, method } : { id, method, params });
      } catch (e) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(new NativeError(e.message, { offline: true }));
      }
    });
  }

  /**
   * Call a method. `start`, `add` and `probe` go straight to the host (which starts YAD);
   * afterwards we say hello so the state turns connected. Everything else needs YAD running.
   */
  async call(method, params, { timeout } = {}) {
    if (STARTS_YAD.has(method)) {
      this.ensurePort();
      const result = await this.send(method, params, timeout ?? START_TIMEOUT);
      if (!this.connected) this.connect({ force: true }).catch(() => {});
      return result;
    }
    await this.connect();
    return this.send(method, params, timeout ?? CALL_TIMEOUT);
  }

  disconnect() {
    this.autoReconnect = false;
    clearTimeout(this.retryTimer);
    this.retryTimer = null;
    if (this.port) { const p = this.port; try { p.disconnect(); } catch { /* ignore */ } this.handleDisconnect(p, 'Disconnected'); }
  }
}
