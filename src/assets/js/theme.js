// Theme = settings.theme. "system" follows prefers-color-scheme; otherwise forced.
// Applied as the `dark` class on <html>; the last choice is cached to avoid a flash on start.
const KEY = 'yad.theme';
const mq = window.matchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null;
let theme = 'system';
try { theme = localStorage.getItem(KEY) || 'system'; } catch { /* storage unavailable */ }

function paint() {
  const dark = theme === 'dark' || (theme !== 'light' && !!(mq && mq.matches));
  document.documentElement.classList.toggle('dark', dark);
  document.documentElement.dataset.theme = theme;
}

// Preview/test override: ?theme=dark|light wins over the setting.
const q = new URLSearchParams(location.search).get('theme');
const forced = q === 'dark' || q === 'light' ? q : null;
if (forced) theme = forced;

export function setTheme(t) {
  if (forced) return;
  theme = t === 'light' || t === 'dark' ? t : 'system';
  try { localStorage.setItem(KEY, theme); } catch { /* ignore */ }
  paint();
}
if (mq && mq.addEventListener) mq.addEventListener('change', paint);
paint();
