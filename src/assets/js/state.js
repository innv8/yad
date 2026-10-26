// In-memory app state plus a tiny event bus. Views subscribe to the topics they draw.

export const state = {
  downloads: new Map(), // id → Download
  settings: null,
  status: null,
  filter: 'all',        // all | active | paused | completed | failed | type:<FileType>
  selected: null,       // download id
  drawerId: null,       // download id shown in the details drawer
  view: 'downloads',    // downloads | settings
  settingsPage: 'general',
  hidden: new Set(),    // ids removed in the UI but not yet sent to the daemon (undo window)
  connected: true,
  menuFor: null,        // id whose ⋯ menu is open
  loaded: false,        // first `list` arrived
};

const subs = new Map();
export function on(topic, cb) {
  if (!subs.has(topic)) subs.set(topic, new Set());
  subs.get(topic).add(cb);
  return () => subs.get(topic).delete(cb);
}
export function emit(topic, data) {
  for (const cb of subs.get(topic) || []) {
    try { cb(data); } catch (e) { console.error(e); }
  }
}

export const STATUS_FILTERS = {
  all: { label: 'All downloads', match: () => true },
  active: { label: 'Active', match: (d) => d.status === 'Downloading' || d.status === 'Queued' },
  paused: { label: 'Paused', match: (d) => d.status === 'Paused' },
  completed: { label: 'Completed', match: (d) => d.status === 'Completed' },
  failed: { label: 'Failed', match: (d) => d.status === 'Failed' || d.status === 'Cancelled' },
};
export const TYPES = ['Videos', 'Compressed', 'Documents', 'Programs', 'Audio', 'Images', 'Others'];
/** Types always listed in the sidebar; the rest appear only when they have files. */
export const CORE_TYPES = ['Videos', 'Compressed', 'Documents', 'Programs', 'Audio'];

export function filterMatch(filter, d) {
  if (filter.startsWith('type:')) return d.fileType === filter.slice(5) || (filter === 'type:Others' && !TYPES.includes(d.fileType));
  return (STATUS_FILTERS[filter] || STATUS_FILTERS.all).match(d);
}
export function filterLabel(filter) {
  if (filter.startsWith('type:')) return filter.slice(5);
  return (STATUS_FILTERS[filter] || STATUS_FILTERS.all).label;
}

/** Downloads the user can see (pending removals excluded), newest first. */
export function visibleDownloads() {
  const out = [];
  for (const d of state.downloads.values()) if (!state.hidden.has(d.id)) out.push(d);
  out.sort((a, b) => (b.createdAt - a.createdAt) || (b.id - a.id));
  return out;
}

export function setDownloads(list) {
  state.downloads = new Map((list || []).map((d) => [d.id, d]));
  state.loaded = true;
  emit('downloads');
}
export function upsert(d) {
  if (!d || d.id == null) return;
  const prev = state.downloads.get(d.id);
  state.downloads.set(d.id, prev ? Object.assign(prev, d) : { ...d });
  emit('downloads');
}
export function drop(id) {
  state.downloads.delete(id);
  state.hidden.delete(id);
  if (state.selected === id) state.selected = null;
  if (state.drawerId === id) { state.drawerId = null; emit('drawer'); }
  emit('downloads');
}

/** Apply a `yad-event` payload. */
export function applyEvent(ev) {
  if (!ev || !ev.type) return;
  switch (ev.type) {
    case 'added':
    case 'updated':
      upsert(ev.download);
      break;
    case 'removed':
      drop(ev.id);
      break;
    case 'progress': {
      const changed = [];
      for (const it of ev.items || []) {
        const d = state.downloads.get(it.id);
        if (!d) continue;
        d.downloaded = it.downloaded;
        d.totalSize = it.totalSize;
        d.speed = it.speed;
        d.eta = it.eta;
        d.connections = it.connections;
        changed.push(d);
      }
      if (changed.length) emit('progress', changed);
      break;
    }
    case 'settings':
      state.settings = ev.settings;
      emit('settings');
      break;
    case 'extension':
      if (state.status) {
        state.status.extension = { ...(state.status.extension || {}), connected: !!ev.connected, browser: ev.browser ?? null };
        if (ev.connected) state.status.extension.lastSeen = Math.floor(Date.now() / 1000);
      }
      emit('status');
      break;
    default:
      break;
  }
}
