// Formatting helpers and HTML escaping. Every user/server string goes through esc().

const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ESC[c]);

const ua = `${navigator.platform || ''} ${navigator.userAgent || ''}`;
export const isMac = /Mac|iPhone|iPad/i.test(ua);
/** Label of the primary modifier: ⌘ on macOS, Ctrl elsewhere (the only per-OS difference). */
export const MOD = isMac ? '⌘' : 'Ctrl';
export const modKey = (e) => (isMac ? e.metaKey : e.ctrlKey);

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];
export function bytes(n) {
  if (n == null || !isFinite(n)) return '—';
  if (n < 1024) return `${Math.max(0, Math.round(n))} B`;
  let i = 0;
  let v = n;
  while (v >= 1024 && i < UNITS.length - 1) { v /= 1024; i++; }
  const s = v < 10 ? v.toFixed(1) : Math.round(v).toString();
  return `${s.replace(/\.0$/, '')} ${UNITS[i]}`;
}
export const speed = (n) => `${bytes(n || 0)}/s`;

export function etaShort(s) {
  if (s == null || !isFinite(s)) return '';
  s = Math.max(0, Math.round(s));
  if (s < 60) return `${s} s left`;
  if (s < 3600) return `${Math.round(s / 60)} min left`;
  const h = Math.floor(s / 3600);
  const m = Math.round((s % 3600) / 60);
  return m ? `${h} h ${m} min left` : `${h} h left`;
}
export function etaLong(s) {
  if (s == null || !isFinite(s)) return 'unknown';
  s = Math.max(0, Math.round(s));
  if (s < 60) return 'less than a minute';
  if (s < 3600) { const m = Math.round(s / 60); return `about ${m} minute${m === 1 ? '' : 's'}`; }
  const h = Math.round(s / 360) / 10;
  return `about ${h} hour${h === 1 ? '' : 's'}`;
}

export const pct = (d) => (d.totalSize ? Math.min(100, Math.floor((d.downloaded / d.totalSize) * 100)) : null);

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const pad = (n) => String(n).padStart(2, '0');
const startOfDay = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
/** 'today' | 'yesterday' | 'earlier' for a unix-seconds timestamp. */
export function dayBucket(ts) {
  if (!ts) return 'earlier';
  const today = startOfDay(new Date());
  const t = ts * 1000;
  if (t >= today) return 'today';
  if (t >= today - 86400000) return 'yesterday';
  return 'earlier';
}
export const clock = (ts) => { const d = new Date(ts * 1000); return `${pad(d.getHours())}:${pad(d.getMinutes())}`; };
export function shortDate(ts) {
  const d = new Date(ts * 1000);
  const s = `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return d.getFullYear() === new Date().getFullYear() ? s : `${s} ${d.getFullYear()}`;
}
/** Completed-row time: "00:31", "Yesterday", "24 Oct". */
export function whenLabel(ts) {
  if (!ts) return '';
  const b = dayBucket(ts);
  return b === 'today' ? clock(ts) : b === 'yesterday' ? 'Yesterday' : shortDate(ts);
}
/** Drawer time: "Today, 01:29". */
export function whenLong(ts) {
  if (!ts) return '—';
  const b = dayBucket(ts);
  return `${b === 'today' ? 'Today' : b === 'yesterday' ? 'Yesterday' : shortDate(ts)}, ${clock(ts)}`;
}
export function ago(ts) {
  if (!ts) return 'never';
  const s = Math.max(0, Date.now() / 1000 - ts);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return shortDate(ts);
}

export function host(url) {
  try { return new URL(url).hostname.replace(/^www\./, ''); } catch { return ''; }
}
export const stripScheme = (url) => String(url || '').replace(/^[a-z]+:\/\//i, '');

/** "/Users/sam/Downloads/Yad" → "~/Downloads/Yad" */
export function tildify(p) {
  if (!p) return '';
  return String(p)
    .replace(/^\/(Users|home)\/[^/]+/, '~')
    .replace(/^[A-Za-z]:\\Users\\[^\\]+/, '~');
}
/** "/Users/sam/Downloads/Yad/Videos" → "Downloads / Yad / Videos" */
export function crumbs(p) {
  const t = tildify(p).replace(/^~[\\/]?/, '');
  return t.split(/[\\/]+/).filter(Boolean).join(' / ') || tildify(p);
}

/** Pull http(s)/ftp links out of pasted or dropped text. */
export function extractUrls(text) {
  if (!text) return [];
  const out = [];
  for (const raw of String(text).split(/[\s<>"']+/)) {
    const s = raw.trim().replace(/[),.;]+$/, '');
    if (/^(https?|ftp):\/\/[^\s]+$/i.test(s)) {
      try { out.push(new URL(s).href); } catch { /* ignore */ }
    }
  }
  return [...new Set(out)];
}

export function errText(e) {
  if (!e) return 'Something went wrong';
  if (typeof e === 'string') return e;
  return e.message || (e.error && e.error.message) || String(e);
}
