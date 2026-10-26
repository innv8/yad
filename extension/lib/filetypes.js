// File type mapping. Mirrors YAD's own mapping (Settings › Browser › "Only for these types").

export const FILE_TYPES = {
  Videos: ['mp4', 'mkv', 'avi', 'mov', 'flv', 'webm', 'wmv', 'mpeg', 'mpg', '3gp'],
  Compressed: ['zip', 'rar', '7z', 'tar', 'gz', 'iso', 'xz'],
  Audio: ['mp3', 'flac', 'wav', 'aac', 'ogg', 'm4a', 'wma', 'opus'],
  Documents: ['pdf', 'docx', 'doc', 'txt', 'xlsx', 'pptx', 'ppt', 'odt', 'epub', 'csv'],
  Programs: ['exe', 'msi', 'apk', 'dmg', 'deb', 'rpm', 'pkg', 'appimage'],
  Images: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg'],
};

const BY_EXT = new Map();
for (const [type, exts] of Object.entries(FILE_TYPES)) for (const e of exts) BY_EXT.set(e, type);

/** Every extension YAD knows about, used to scan pages for file links. */
export const KNOWN_EXTS = [...BY_EXT.keys()];

/** Extensions that are plausibly whole media files (used with application/octet-stream). */
export const MEDIA_EXTS = new Set(['mp4', 'm4v', 'mkv', 'mov', 'webm', 'avi', 'flv', 'wmv', 'mpg', 'mpeg', '3gp',
  'mp3', 'm4a', 'flac', 'wav', 'ogg', 'oga', 'opus', 'wma']);

/** Stream segment extensions: never offered on their own. */
export const SEGMENT_EXTS = new Set(['ts', 'm4s', 'm4f', 'cmfv', 'cmfa', 'aac', 'vtt', 'webvtt', 'key', 'init']);

/** Lower-case extension of a URL path or file name, '' when none. */
export function extOf(urlOrName) {
  if (!urlOrName) return '';
  let path = String(urlOrName);
  try { path = new URL(path).pathname; } catch { path = path.split(/[?#]/)[0]; }
  const base = path.split('/').pop() || '';
  const dot = base.lastIndexOf('.');
  if (dot <= 0) return '';
  const ext = base.slice(dot + 1).toLowerCase();
  return /^[a-z0-9]{1,8}$/.test(ext) ? ext : '';
}

const MIME_TYPES = [
  [/^video\//, 'Videos'],
  [/^audio\//, 'Audio'],
  [/^image\//, 'Images'],
  [/^application\/(pdf|msword|epub\+zip|rtf|vnd\.(openxmlformats|ms-excel|ms-powerpoint|oasis\.opendocument))/, 'Documents'],
  [/^text\/(plain|csv)/, 'Documents'],
  [/^application\/(zip|x-zip|x-rar|vnd\.rar|x-7z|x-tar|gzip|x-gzip|x-xz|x-iso9660|x-bzip)/, 'Compressed'],
  [/^application\/(x-msdownload|x-msi|x-ms-installer|vnd\.android\.package-archive|x-apple-diskimage|x-debian-package|vnd\.debian\.binary-package|x-rpm|x-redhat-package-manager|x-newton-compatible-pkg|x-executable)/, 'Programs'],
];

/** YAD FileType for an extension and/or MIME type. */
export function typeOf(ext, mime) {
  const byExt = BY_EXT.get((ext || '').toLowerCase());
  if (byExt) return byExt;
  const m = (mime || '').toLowerCase().split(';')[0].trim();
  for (const [re, t] of MIME_TYPES) if (re.test(m)) return t;
  return 'Others';
}

/** Icon / tile colour key for a FileType. */
export function tileFor(type) {
  return { Videos: 'video', Audio: 'audio', Compressed: 'zip', Documents: 'doc', Programs: 'app', Images: 'image' }[type] || 'doc';
}
