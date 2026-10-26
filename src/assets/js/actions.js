// User actions shared by rows, the row menu, the details drawer and keyboard shortcuts.
import { call, openPath, revealPath } from './api.js';
import { state, upsert, emit } from './state.js';
import { showToast, toastError } from './toast.js';
import { openMenu } from './menu.js';
import { openAddSheet, confirmSheet } from './sheet.js';
import { startRename } from './list.js';

const apply = (d) => { if (d && typeof d === 'object' && d.id != null) upsert(d); };
const run = (method, d) => call(method, { id: d.id }).then(apply, toastError);

export const pause = (d) => run('pause', d);
export const resume = (d) => run('resume', d);
export const retry = (d) => run('retry', d);
export const cancel = (d) => run('cancel', d);

export const isActive = (d) => d.status === 'Downloading';
export const isDone = (d) => d.status === 'Completed';
export const isBroken = (d) => d.status === 'Failed' || d.status === 'Cancelled';

/** The row's one primary action: pause / resume / retry / show in folder. */
export function primary(d) {
  switch (d.status) {
    case 'Downloading':
    case 'Queued': return pause(d);
    case 'Paused': return resume(d);
    case 'Failed':
    case 'Cancelled': return retry(d);
    case 'Completed': return reveal(d);
    default: return undefined;
  }
}
export const primaryLabel = (d) => ({
  Downloading: 'Pause', Queued: 'Pause', Paused: 'Resume', Failed: 'Retry', Cancelled: 'Retry', Completed: 'Show in folder',
}[d.status] || '');
export const primaryIcon = (d) => ({
  Downloading: 'pause', Queued: 'pause', Paused: 'play', Failed: 'retry', Cancelled: 'retry', Completed: 'folder',
}[d.status] || 'more');

/** Space: pause / resume only. */
export function togglePause(d) {
  if (d.status === 'Downloading' || d.status === 'Queued') return pause(d);
  if (d.status === 'Paused') return resume(d);
  return undefined;
}

export const open = (d) => openPath(d.destinationPath).catch(toastError);
export const reveal = (d) => revealPath(d.destinationPath || d.destinationDir).catch(toastError);
/** Enter: open a finished file, otherwise the primary action. */
export const enter = (d) => (isDone(d) ? open(d) : primary(d));

export async function copyLink(d) {
  try {
    const cm = window.__TAURI__ && window.__TAURI__.clipboardManager;
    if (cm && cm.writeText) await cm.writeText(d.url);
    else await navigator.clipboard.writeText(d.url);
    showToast('Link copied', { duration: 2000 });
  } catch {
    // Fallback for web views without async clipboard permission.
    const ta = document.createElement('textarea');
    ta.value = d.url;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand('copy');
    ta.remove();
    showToast(ok ? 'Link copied' : "Couldn't copy the link", { duration: 2000 });
  }
}

export function downloadAgain(d) {
  return call('add', { url: d.url, fileName: d.fileName, directory: d.destinationDir, source: 'app' })
    .then(apply, toastError);
}

/** Hide rows now, send `remove` when the undo toast expires. */
export function removeWithUndo(list) {
  const items = list.filter((d) => d && !state.hidden.has(d.id));
  if (!items.length) return;
  for (const d of items) state.hidden.add(d.id);
  if (items.some((d) => d.id === state.drawerId)) { state.drawerId = null; emit('drawer'); }
  emit('downloads');
  const text = items.length === 1
    ? `Removed ${items[0].fileName} from the list`
    : `Removed ${items.length} downloads from the list`;
  showToast(text, {
    action: 'Undo',
    duration: 6000,
    onAction() {
      for (const d of items) state.hidden.delete(d.id);
      emit('downloads');
    },
    onExpire() {
      for (const d of items) {
        call('remove', { id: d.id }).catch((e) => {
          state.hidden.delete(d.id);
          emit('downloads');
          toastError(e);
        });
      }
    },
  });
}

export async function deleteFile(d) {
  const ok = await confirmSheet({
    title: 'Delete file?',
    text: `${d.fileName} will be removed from the list and deleted from your computer.`,
    ok: 'Delete file',
    danger: true,
  });
  if (!ok) return;
  state.hidden.add(d.id);
  emit('downloads');
  call('deleteFile', { id: d.id }).catch((e) => {
    state.hidden.delete(d.id);
    emit('downloads');
    toastError(e);
  });
}

/** Add one or more links. Asks first (New download sheet) when the setting says so. */
export async function addUrls(urls, source = 'app') {
  if (!urls.length) return;
  if (state.settings && state.settings.askBeforeDownload && urls.length === 1) {
    openAddSheet(urls[0], source);
    return;
  }
  const results = await Promise.allSettled(urls.map((url) => call('add', { url, source })));
  let added = 0;
  let existing = null;
  for (const r of results) {
    if (r.status !== 'fulfilled' || !r.value) continue;
    // `add` returns the existing download for a link that is already queued/downloading/paused.
    if (state.downloads.has(r.value.id) && !state.hidden.has(r.value.id)) existing = r.value; else added++;
    apply(r.value);
  }
  const failed = results.find((r) => r.status === 'rejected');
  if (failed) toastError(failed.reason);
  else if (added > 1) showToast(`Added ${added} downloads`, { duration: 3000 });
  else if (existing && !added) showToast(`${existing.fileName} is already in the list`, { duration: 3000 });
}

/** The row menu (right-click or ⋯). */
export function rowMenu(d, where) {
  const items = [];
  if (isDone(d)) {
    items.push({ icon: 'doc', label: 'Open', kbd: 'Enter', action: () => open(d) });
    items.push({ icon: 'folder', label: 'Show in folder', action: () => reveal(d) });
  } else if (d.status === 'Downloading' || d.status === 'Queued') {
    items.push({ icon: 'pause', label: 'Pause', kbd: 'Space', action: () => pause(d) });
  } else if (d.status === 'Paused') {
    items.push({ icon: 'play', label: 'Resume', kbd: 'Space', action: () => resume(d) });
  } else if (isBroken(d)) {
    items.push({ icon: 'retry', label: 'Retry', kbd: 'Enter', action: () => retry(d) });
  }
  items.push({ icon: 'copy', label: 'Copy link', action: () => copyLink(d) });
  items.push({ icon: 'edit', label: 'Rename…', kbd: 'F2', action: () => startRename(d.id) });
  if (isDone(d) || isBroken(d)) items.push({ icon: 'retry', label: 'Download again', action: () => downloadAgain(d) });
  if (!isDone(d) && !isBroken(d)) items.push({ icon: 'x', label: 'Cancel download', action: () => cancel(d) });
  items.push('sep');
  items.push({ icon: 'x', label: 'Remove from list', kbd: 'Del', action: () => removeWithUndo([d]) });
  items.push({ icon: 'trash', label: 'Delete file', danger: true, action: () => deleteFile(d) });
  state.menuFor = d.id;
  emit('menu');
  return openMenu({
    ...where,
    items,
    onClose() { state.menuFor = null; emit('menu'); },
  });
}
