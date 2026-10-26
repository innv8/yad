// Sidebar: status + type filters with live counts (downloads view) or the settings nav.
import { state, on, emit, visibleDownloads, filterMatch, STATUS_FILTERS, TYPES, CORE_TYPES } from './state.js';
import { icon } from './icons.js';
import { PAGES } from './settings.js';

const STATUS_ICONS = { all: 'down', active: 'bolt', paused: 'pause', completed: 'check', failed: 'retry' };
const TYPE_DOT = { Videos: 'v', Compressed: 'z', Documents: 'd', Programs: 'a', Audio: 'm', Images: 'p', Others: 'o' };

let els = {};

export function initSidebar(aside, { onSettingsPage, onBack }) {
  els.aside = aside;
  aside.innerHTML = `
    <nav class="nav" data-nav="filters">${Object.entries(STATUS_FILTERS).map(([k, f]) => `<button data-filter="${k}">${icon(STATUS_ICONS[k])}${f.label}<span class="n">0</span></button>`).join('')}</nav>
    <nav class="nav" data-nav="types"><h6>Types</h6>${TYPES.map((t) => `<button data-filter="type:${t}"><span class="dot d-${TYPE_DOT[t]}"></span>${t}<span class="n">0</span></button>`).join('')}</nav>
    <nav class="nav snav" data-nav="back" hidden><button data-a="back">${icon('back')}Downloads</button></nav>
    <nav class="nav" data-nav="settings" hidden><h6>Settings</h6>${PAGES.map(([k, i, l]) => `<button data-page="${k}">${icon(i)}${l}</button>`).join('')}</nav>
    <div class="svc"><div class="svc-daemon"><span class="live"></span><span class="t"></span></div><div class="svc-ext">${icon('globe')}<span class="t"></span></div></div>`;
  aside.addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (!b) return;
    if (b.dataset.filter) {
      state.filter = b.dataset.filter;
      emit('filter');
      renderSidebar();
    } else if (b.dataset.page) onSettingsPage(b.dataset.page);
    else if (b.dataset.a === 'back') onBack();
  });
  for (const t of ['downloads', 'filter', 'view', 'status', 'connection']) on(t, renderSidebar);
  renderSidebar();
}

export function renderSidebar() {
  const a = els.aside;
  if (!a) return;
  const settings = state.view === 'settings';
  a.querySelector('[data-nav="filters"]').hidden = settings;
  a.querySelector('[data-nav="types"]').hidden = settings;
  a.querySelector('[data-nav="back"]').hidden = !settings;
  a.querySelector('[data-nav="settings"]').hidden = !settings;

  const all = visibleDownloads();
  for (const b of a.querySelectorAll('[data-filter]')) {
    const f = b.dataset.filter;
    const n = all.filter((d) => filterMatch(f, d)).length;
    const nEl = b.querySelector('.n');
    if (nEl.textContent !== String(n)) nEl.textContent = n;
    b.classList.toggle('on', state.filter === f);
    if (f.startsWith('type:')) b.hidden = !CORE_TYPES.includes(f.slice(5)) && n === 0 && state.filter !== f;
  }
  for (const b of a.querySelectorAll('[data-page]')) b.classList.toggle('on', state.settingsPage === b.dataset.page);

  const on = state.connected && !!state.status;
  a.querySelector('.svc-daemon .live').classList.toggle('off', !on);
  a.querySelector('.svc-daemon .t').textContent = on ? 'Background service on' : 'Background service off';
  const linked = on && !!(state.status.extension && state.status.extension.connected);
  const ext = a.querySelector('.svc-ext');
  ext.querySelector('.t').textContent = linked ? 'Extension linked' : 'Extension not linked';
  ext.classList.toggle('off-text', !linked);
}
