// ── State ──────────────────────────────────────────────────────────
const state = {
  records: [],
  selected: new Set(),
  sortColumn: '',
  sortDir: 'asc',
  filterText: '',
  activeDownloads: new Map(), // downloadId → { timestamp, bytes, speed, eta }
  customDir: '',
  pendingUrl: '', // URL waiting for rename confirmation
};

// ── Utilities ──────────────────────────────────────────────────────

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const listen = (ev, cb) => window.__TAURI__.event.listen(ev, cb);

function log(m) { console.log(`${Date.now()}: ${m}`); }

function getSize(s) {
  if (!s || s === 0) return '0 B';
  const u = ['Bytes', 'KB', 'MB', 'GB'];
  let i = 0;
  let v = s;
  while (v >= 1024 && i < 3) { v /= 1024; i++; }
  return `${Math.round(v * 10) / 10} ${u[i]}`;
}

function formatTime(ts) {
  if (!ts || ts === 0) return '—';
  const d = new Date(ts * 1000);
  return d.toLocaleString('en-GB', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

function statusLabel(s) {
  const m = { Finished: 'Complete', InProgress: 'Downloading', Failed: 'Failed', Pending: 'Pending', Cancelled: 'Cancelled' };
  return m[s] || s;
}

function statusBadge(s) {
  const cls = ({ Finished: 'finished', InProgress: 'inprogress', Failed: 'failed', Pending: 'pending', Cancelled: 'cancelled' })[s] || 'pending';
  return `<span class="status-badge ${cls}">${statusLabel(s)}</span>`;
}

function isUrl(str) { return /^https?:\/\/.+/i.test(str.trim()); }

// ── Core rendering ─────────────────────────────────────────────────

async function getRecords() {
  try {
    state.records = await invoke('fetch_records') || [];
  } catch (e) {
    log(`fetch_records error: ${e}`);
    state.records = [];
  }
  render();
}

function filterRows() {
  const q = state.filterText.toLowerCase();
  return q ? state.records.filter(r => r.file_name.toLowerCase().includes(q) || (r.file_url || '').toLowerCase().includes(q)) : state.records;
}

function sortRows(rows) {
  if (!state.sortColumn) return rows;
  const c = state.sortColumn;
  const d = state.sortDir === 'asc' ? 1 : -1;
  return [...rows].sort((a, b) => {
    let va = a[c], vb = b[c];
    if (c === 'file_size') { va = Number(va) || 0; vb = Number(vb) || 0; return (va - vb) * d; }
    if (c === 'progress') {
      const pA = a.download_status === 'Finished' ? 100 : a.downloaded_percentage || 0;
      const pB = b.download_status === 'Finished' ? 100 : b.downloaded_percentage || 0;
      return (pA - pB) * d;
    }
    va = String(va || '').toLowerCase();
    vb = String(vb || '').toLowerCase();
    return va < vb ? -d : va > vb ? d : 0;
  });
}

function render() {
  const tbody = document.getElementById('download-records');
  const empty = document.getElementById('empty-state');

  // Build sorted & filtered rows
  let rows = filterRows();
  rows = sortRows(rows);

  if (rows.length === 0) {
    tbody.innerHTML = '';
    empty.style.display = 'block';
    updateStats();
    updateTitle();
    return;
  }
  empty.style.display = 'none';

  let html = '';
  for (const r of rows) {
    const sel = state.selected.has(r.id) ? ' checked' : '';
    const status = r.download_status;
    const pct = status === 'Finished' ? 100 : status === 'Pending' ? 0 : Math.round(r.downloaded_percentage || 0);
    const barCls = status === 'Finished' ? 'success' : status === 'InProgress' ? 'info' : status === 'Failed' ? 'danger' : 'warning';
    const pBarCls = status === 'InProgress' ? 'progress-bar-striped progress-bar-animated active-anim' : '';
    const actCls = status === 'Finished' ? 'primary' : status === 'InProgress' ? 'warning' : 'success';
    const icon = status === 'Finished' ? 'fa-folder-open' : status === 'InProgress' ? 'fa-pause' : 'fa-play';

    html += `
      <tr id="row-${r.id}" class="${sel ? 'row-selected' : ''}" tabindex="0" data-id="${r.id}">
        <td class="col-select"><input type="checkbox" class="row-check" data-id="${r.id}"${sel} /></td>
        <td class="col-file"><span class="file-name-cell d-block" title="${escAttr(r.file_name)}">${escHtml(r.file_name)}</span></td>
        <td class="col-size" id="size-${r.id}">${getSize(r.file_size)}</td>
        <td class="col-progress" id="progress-${r.id}">
          <div class="progress" role="progressbar" aria-valuenow="${pct}" aria-valuemax="100">
            <div class="progress-bar text-bg-${barCls} ${pBarCls}" style="width:${pct}%">${pct}%</div>
          </div>
          <div id="speed-${r.id}" class="speed-eta mt-1"></div>
        </td>
        <td class="col-type">${escHtml(r.file_type)}${statusBadge(status)}</td>
        <td class="col-date">${formatTime(r.download_start_time)}</td>
        <td class="col-actions">
          <span class="action-link btn btn-sm btn-outline-${actCls}" data-id="${r.id}" data-url="${escAttr(r.file_url)}" data-status="${status}" data-path="${escAttr(r.destination_path)}" title="${status === 'Finished' ? 'Open file' : status === 'InProgress' ? 'Cancel' : 'Retry download'}"><i class="fa ${icon}"></i></span>
          <span class="delete-link btn btn-sm btn-outline-danger ms-1" data-id="${r.id}" title="Delete record"><i class="fa fa-trash"></i></span>
        </td>
      </tr>`;
  }
  tbody.innerHTML = html;
  attachRowHandlers();
  updateSortIcons();
  updateStats();
  updateTitle();
}

function escHtml(s) { const d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
function escAttr(s) { return String(s).replace(/"/g, '&quot;').replace(/'/g, '&#39;'); }

// ── Sort icons ─────────────────────────────────────────────────────

function updateSortIcons() {
  document.querySelectorAll('#downloads-table th[data-sort]').forEach(th => {
    const col = th.dataset.sort;
    th.classList.remove('sort-asc', 'sort-desc');
    if (col === state.sortColumn) th.classList.add(state.sortDir === 'asc' ? 'sort-asc' : 'sort-desc');
  });
}

// ── Row event handlers ─────────────────────────────────────────────

function attachRowHandlers() {
  // Checkboxes
  document.querySelectorAll('.row-check').forEach(cb => {
    cb.onchange = () => {
      const id = Number(cb.dataset.id);
      if (cb.checked) state.selected.add(id); else state.selected.delete(id);
      document.getElementById(`row-${id}`)?.classList.toggle('row-selected', cb.checked);
      updateBulkBar();
    };
  });

  // Select-all
  const selAll = document.getElementById('select-all');
  selAll.onchange = () => {
    document.querySelectorAll('.row-check').forEach(cb => { cb.checked = selAll.checked; cb.onchange(); });
  };

  // Action links
  document.querySelectorAll('.action-link').forEach(el => {
    el.onclick = () => {
      const id = Number(el.dataset.id);
      const url = el.dataset.url;
      const status = el.dataset.status;
      const path = el.dataset.path;
      if (status === 'Finished') invoke('open_file', { path });
      else if (status === 'InProgress') invoke('cancel_download', { downloadId: id });
      else startDownload(url);
    };
  });

  // Delete links
  document.querySelectorAll('.delete-link').forEach(el => {
    el.onclick = () => deleteRecord(Number(el.dataset.id));
  });

  // Row click → checkbox
  document.querySelectorAll('#download-records tr').forEach(tr => {
    tr.onclick = (e) => {
      if (e.target.closest('.action-link') || e.target.closest('.delete-link') || e.target.closest('input')) return;
      const cb = tr.querySelector('.row-check');
      if (cb) { cb.checked = !cb.checked; cb.onchange(); }
    };
    tr.oncontextmenu = (e) => { e.preventDefault(); showContextMenu(e, Number(tr.dataset.id)); };
    tr.onkeydown = (e) => handleRowKeydown(e, tr);
  });
}

// ── Context menu ───────────────────────────────────────────────────

let contextId = null;

function showContextMenu(e, id) {
  contextId = id;
  const menu = document.getElementById('context-menu');
  const r = state.records.find(x => x.id === id);
  if (!r) return;
  menu.style.display = 'block';
  menu.style.left = `${e.clientX}px`;
  menu.style.top = `${e.clientY}px`;
  // Show/hide items based on status
  menu.querySelectorAll('[data-action]').forEach(item => {
    const a = item.dataset.action;
    if (a === 'cancel') item.style.display = r.download_status === 'InProgress' ? 'block' : 'none';
    else if (a === 'retry') item.style.display = ['Failed', 'Cancelled', 'Pending'].includes(r.download_status) ? 'block' : 'none';
    else if (a === 'open') item.style.display = r.download_status === 'Finished' ? 'block' : 'none';
    else item.style.display = 'block';
  });
}

document.getElementById('context-menu').addEventListener('click', async (e) => {
  const item = e.target.closest('.context-item');
  if (!item || !contextId) return;
  const r = state.records.find(x => x.id === contextId);
  if (!r) return;
  const a = item.dataset.action;
  if (a === 'open') await invoke('open_file', { path: r.destination_path });
  else if (a === 'open-folder') await invoke('open_file', { path: r.destination_dir });
  else if (a === 'copy-url') navigator.clipboard.writeText(r.file_url);
  else if (a === 'retry') await startDownload(r.file_url);
  else if (a === 'cancel') await invoke('cancel_download', { downloadId: r.id });
  else if (a === 'delete') await deleteRecord(r.id);
  hideContextMenu();
});
document.addEventListener('click', hideContextMenu);
function hideContextMenu() { document.getElementById('context-menu').style.display = 'none'; contextId = null; }

// ── Keyboard navigation ────────────────────────────────────────────

function handleRowKeydown(e, tr) {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    const rows = [...document.querySelectorAll('#download-records tr')];
    const idx = rows.indexOf(tr);
    const next = e.key === 'ArrowDown' ? rows[idx + 1] : rows[idx - 1];
    if (next) { next.focus(); next.querySelector('.row-check')?.focus(); }
  } else if (e.key === 'Enter') {
    e.preventDefault();
    tr.querySelector('.action-link')?.click();
  } else if (e.key === 'Delete' || e.key === 'Backspace') {
    e.preventDefault();
    const id = Number(tr.dataset.id);
    if (id) deleteRecord(id);
  } else if (e.key === ' ') {
    e.preventDefault();
    const cb = tr.querySelector('.row-check');
    if (cb) { cb.checked = !cb.checked; cb.onchange(); }
  }
}

// ── Bulk bar ───────────────────────────────────────────────────────

function updateBulkBar() {
  const bar = document.getElementById('bulk-bar');
  const count = state.selected.size;
  document.getElementById('selected-count').textContent = `${count} selected`;
  bar.style.display = count > 0 ? 'flex' : 'none';
  bar.classList.toggle('show', count > 0);
}

document.getElementById('retry-selected-btn').onclick = async () => {
  const ids = [...state.selected];
  for (const id of ids) {
    const r = state.records.find(x => x.id === id);
    if (r && ['Failed', 'Cancelled', 'Pending'].includes(r.download_status)) await startDownload(r.file_url);
  }
  state.selected.clear();
  updateBulkBar();
};
document.getElementById('delete-selected-btn').onclick = async () => {
  if (!confirm(`Delete ${state.selected.size} record(s)?`)) return;
  for (const id of state.selected) await deleteRecord(id);
  state.selected.clear();
  updateBulkBar();
};

// ── Delete & Clear completed ───────────────────────────────────────

async function deleteRecord(id) {
  if (!confirm('Delete this download record?')) return;
  try { await invoke('delete_record', { id }); } catch (e) { log(`delete error: ${e}`); }
  state.selected.delete(id);
  await getRecords();
}

document.getElementById('clear-completed-btn').onclick = async () => {
  const completed = state.records.filter(r => r.download_status === 'Finished');
  if (completed.length === 0) return;
  if (!confirm(`Delete ${completed.length} completed record(s)?`)) return;
  for (const r of completed) { try { await invoke('delete_record', { id: r.id }); } catch (_) {} }
  await getRecords();
};

// ── Stats ──────────────────────────────────────────────────────────

function updateStats() {
  const total = state.records.length;
  const finished = state.records.filter(r => r.download_status === 'Finished').length;
  const failed = state.records.filter(r => r.download_status === 'Failed').length;
  const active = state.records.filter(r => r.download_status === 'InProgress').length;
  const parts = [`${total} total`];
  if (finished) parts.push(`${finished} completed`);
  if (failed) parts.push(`${failed} failed`);
  if (active) parts.push(`${active} active`);
  document.getElementById('stats-text').textContent = parts.join(' · ');
  document.getElementById('clear-completed-btn').style.display = finished > 0 ? '' : 'none';
}

// ── Title ──────────────────────────────────────────────────────────

function updateTitle() {
  const active = state.records.filter(r => r.download_status === 'InProgress').length;
  const base = 'YAD';
  document.title = active > 0 ? `(${active}) ${base}` : base;
}

// ── Speed & ETA ────────────────────────────────────────────────────

function updateSpeed(id, downloaded, total, timestamp) {
  const now = Date.now();
  let info = state.activeDownloads.get(id);
  if (!info) {
    info = { lastBytes: downloaded, lastTime: now, started: now, speed: 0 };
    state.activeDownloads.set(id, info);
    return;
  }
  const dt = (now - info.lastTime) / 1000;
  if (dt < 0.5) return;
  const dBytes = downloaded - info.lastBytes;
  info.speed = dBytes / dt;
  info.lastBytes = downloaded;
  info.lastTime = now;
  state.activeDownloads.set(id, info);

  const speed = info.speed;
  const remaining = total - downloaded;
  const eta = speed > 0 ? remaining / speed : 0;

  const el = document.getElementById(`speed-${id}`);
  if (!el) return;
  const speedStr = speed > 1024 * 1024 ? `${(speed / 1024 / 1024).toFixed(1)} MB/s` : speed > 1024 ? `${(speed / 1024).toFixed(1)} KB/s` : `${speed.toFixed(0)} B/s`;
  if (remaining > 0 && speed > 0) {
    const etaStr = eta > 3600 ? `${(eta / 3600).toFixed(1)}h` : eta > 60 ? `${(eta / 60).toFixed(1)}m` : `${eta.toFixed(0)}s`;
    el.textContent = `${speedStr} · ETA ${etaStr}`;
  } else {
    el.textContent = speedStr;
  }
}

function clearSpeed(id) {
  state.activeDownloads.delete(id);
  const el = document.getElementById(`speed-${id}`);
  if (el) el.textContent = '';
}

// ── Download flow ──────────────────────────────────────────────────

async function startDownload(url, customName, customDir) {
  try {
    await invoke('download', { url, fileName: customName || null, destinationDir: customDir || null });
  } catch (e) {
    log(`Download error: ${e}`);
    showAlert(`Download failed: ${e}`, 'danger');
  }
}

// File rename modal
let renameResolve = null;

function promptFileName(url) {
  const name = url.split('/').filter(s => s).pop() || 'download';
  document.getElementById('rename-input').value = name;
  const modal = document.getElementById('rename-modal');
  modal.style.display = 'block';
  modal.classList.add('show');
  document.body.classList.add('modal-open');
  const backdrop = document.createElement('div');
  backdrop.className = 'modal-backdrop fade show';
  backdrop.id = 'rename-backdrop';
  document.body.appendChild(backdrop);

  return new Promise(resolve => {
    renameResolve = resolve;
  });
}

document.getElementById('rename-confirm').onclick = () => {
  const val = document.getElementById('rename-input').value.trim();
  closeRenameModal();
  if (renameResolve) renameResolve(val || null);
};

document.querySelectorAll('#rename-modal .btn-close, #rename-modal [data-bs-dismiss="modal"]').forEach(el => {
  el.onclick = () => {
    closeRenameModal();
    if (renameResolve) renameResolve(null);
  };
});

function closeRenameModal() {
  const modal = document.getElementById('rename-modal');
  modal.style.display = 'none';
  modal.classList.remove('show');
  document.body.classList.remove('modal-open');
  document.getElementById('rename-backdrop')?.remove();
}

// ── URL input ──────────────────────────────────────────────────────

const urlInput = document.getElementById('search');

// Paste → auto-download with rename prompt
urlInput.addEventListener('paste', async (e) => {
  const text = e.clipboardData.getData('text');
  const urls = text.split('\n').map(s => s.trim()).filter(isUrl);
  if (urls.length === 0) { showAlert('No valid URLs found in paste.', 'warning'); return; }
  urlInput.value = '';
  for (const u of urls) {
    const name = await promptFileName(u);
    if (name) await startDownload(u, name, state.customDir || null);
    else await startDownload(u, null, state.customDir || null);
  }
});

// Enter key
urlInput.addEventListener('keydown', async (e) => {
  if (e.key !== 'Enter') return;
  e.preventDefault();
  const url = urlInput.value.trim();
  if (!isUrl(url)) { showAlert('Invalid URL. Must start with http:// or https://.', 'warning'); return; }
  urlInput.value = '';
  const name = await promptFileName(url);
  if (name) await startDownload(url, name, state.customDir || null);
  else await startDownload(url, null, state.customDir || null);
});

// Download button
document.getElementById('download-btn').onclick = async () => {
  const url = urlInput.value.trim();
  if (!isUrl(url)) { showAlert('Invalid URL. Must start with http:// or https://.', 'warning'); return; }
  urlInput.value = '';
  const name = await promptFileName(url);
  if (name) await startDownload(url, name, state.customDir || null);
  else await startDownload(url, null, state.customDir || null);
};

// ── Directory picker ───────────────────────────────────────────────

document.getElementById('pick-dir-btn').onclick = async () => {
  try {
    const selected = await window.__TAURI__.dialog.open({ directory: true, title: 'Choose download folder' });
    if (selected) {
      state.customDir = selected;
      document.getElementById('dir-label').textContent = selected.split('/').pop() || selected;
      document.getElementById('dir-label').classList.remove('d-none');
    }
  } catch (e) {
    log(`Dialog picker error: ${e}`);
    const dir = prompt('Enter download directory path (leave empty for default):');
    if (dir) {
      state.customDir = dir;
      document.getElementById('dir-label').textContent = dir.split('/').pop() || dir;
      document.getElementById('dir-label').classList.remove('d-none');
    }
  }
};

// ── Filter ─────────────────────────────────────────────────────────

document.getElementById('filter-input').addEventListener('input', (e) => {
  state.filterText = e.target.value;
  render();
});

// ── Sort ───────────────────────────────────────────────────────────

document.querySelectorAll('#downloads-table th[data-sort]').forEach(th => {
  th.onclick = () => {
    const col = th.dataset.sort;
    if (col === 'none') return;
    if (state.sortColumn === col) state.sortDir = state.sortDir === 'asc' ? 'desc' : 'asc';
    else { state.sortColumn = col; state.sortDir = 'asc'; }
    render();
  };
});

// ── Alerts ─────────────────────────────────────────────────────────

function showAlert(msg, type = 'danger') {
  const container = document.querySelector('main.container');
  const id = 'live-alert';
  let el = document.getElementById(id);
  if (!el) {
    el = document.createElement('div');
    el.id = id;
    el.className = 'alert alert-dismissible fade show mt-2';
    el.innerHTML = '<span id="live-alert-text"></span><button type="button" class="btn-close" data-bs-dismiss="alert"></button>';
    container.insertBefore(el, container.firstChild);
  }
  el.className = `alert alert-${type} alert-dismissible fade show mt-2`;
  document.getElementById('live-alert-text').textContent = msg;
  el.style.display = 'block';
  clearTimeout(el._timer);
  el._timer = setTimeout(() => el.style.display = 'none', 6000);
}

// ── Tauri events ───────────────────────────────────────────────────

listen('download-started', (e) => {
  const d = e.payload;
  log(`download-started: ${d.downloadId}`);
  getRecords();
});

listen('download-progress', (e) => {
  const d = e.payload;
  const id = d.downloadId;
  const pct = d.totalSize > 0 ? Math.min(100, (d.downloaded / d.totalSize) * 100) : 0;
  const intPct = Math.round(pct);

  // Progress bar
  const pc = document.getElementById(`progress-${id}`);
  if (pc) {
    pc.innerHTML = `
      <div class="progress" role="progressbar" aria-valuenow="${intPct}" aria-valuemax="100">
        <div class="progress-bar text-bg-info progress-bar-striped progress-bar-animated active-anim" style="width:${intPct}%">${intPct}%</div>
      </div>
      <div id="speed-${id}" class="speed-eta mt-1"></div>`;
  }

  // Size cell
  const sc = document.getElementById(`size-${id}`);
  if (sc) sc.textContent = `${getSize(d.downloaded)} / ${getSize(d.totalSize)}`;

  // Speed & ETA
  updateSpeed(id, d.downloaded, d.totalSize, d.timestamp);

  if (intPct >= 100) {
    clearSpeed(id);
    setTimeout(() => getRecords(), 600);
  }
});

listen('download-message', (e) => {
  const d = e.payload;
  log(`download-message: ${d.status} — ${d.message}`);
  showAlert(d.message, d.status === 'error' ? 'danger' : 'success');
  getRecords();
});

// ── Theme ──────────────────────────────────────────────────────────

window.addEventListener('DOMContentLoaded', () => {
  const dark = document.getElementById('dark-button');
  const light = document.getElementById('light-button');
  const theme = localStorage.getItem('theme');
  if (theme === 'dark') { document.body.classList.add('dark-theme'); dark.style.display = 'none'; }
  else { light.style.display = 'none'; }

  dark.onclick = () => {
    document.body.classList.add('dark-theme');
    localStorage.setItem('theme', 'dark');
    dark.style.display = 'none';
    light.style.display = 'block';
  };
  light.onclick = () => {
    document.body.classList.remove('dark-theme');
    localStorage.setItem('theme', 'light');
    light.style.display = 'none';
    dark.style.display = 'block';
  };
});

// ── Init ───────────────────────────────────────────────────────────

window.onload = () => getRecords();
