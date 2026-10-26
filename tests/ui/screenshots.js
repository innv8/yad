// README screenshots: `npm run screenshots` writes docs/images/*.png.
// The app is rendered by WebKit (the engine behind the macOS app) from the ?mock harness;
// the extension pages by Chromium, with a stand-in for the chrome.* APIs and the background.
import { chromium, webkit } from '@playwright/test';
import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const OUT = path.join(ROOT, 'docs/images');
const TYPES = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.woff2': 'font/woff2', '.png': 'image/png', '.ico': 'image/x-icon', '.svg': 'image/svg+xml', '.webm': 'video/webm',
};

// A video page the extension found: an HLS stream with several qualities.
const VIDEO_PAGE = '/tests/ui/fixtures/video-page.html';
const VIDEO_SRC = '/tests/ui/fixtures/demo.webm';
const media = (origin) => [{
  url: origin + VIDEO_SRC, kind: 'hls', size: null, fileName: 'keynote.m3u8', label: 'HLS', container: 'MP4',
  audio: false, probing: false, error: null, preferred: '1080', seen: 1,
  variants: [
    { id: '1080', label: '1080p', detail: 'H.264 · 6.2 Mb/s', size: 1_480_000_000, audioOnly: false, container: 'MP4' },
    { id: '720', label: '720p', detail: 'H.264 · 3.1 Mb/s', size: 742_000_000, audioOnly: false, container: 'MP4' },
    { id: '480', label: '480p', detail: 'H.264 · 1.4 Mb/s', size: 335_000_000, audioOnly: false, container: 'MP4' },
    { id: 'audio', label: 'Audio only', detail: 'AAC · 128 kb/s', size: 30_500_000, audioOnly: true, container: 'M4A' },
  ],
}];
const links = [
  { url: 'https://cdn.example.com/talks/keynote-slides.pdf', name: 'keynote-slides.pdf', size: 8_400_000 },
  { url: 'https://cdn.example.com/talks/demo-project.zip', name: 'demo-project.zip', size: 52_300_000 },
  { url: 'https://cdn.example.com/talks/transcript.txt', name: 'transcript.txt', size: 96_000 },
];

// Stand-in for chrome.* and the background service worker, as an init script.
function chromeStub({ origin, media, links, tabTitle }) {
  const settings = {
    videoQuality: 'best',
    browser: { videoButton: true, takeOver: true, types: ['Videos', 'Audio', 'Compressed', 'Programs'], minSize: 1048576, excludedSites: [], sendCookies: true, contextMenu: true },
  };
  const state = { connected: true, connecting: false, error: null, status: { version: '2.0.0', active: 1, queued: 0 }, settings };
  const tab = { id: 7, url: 'https://talks.example.com/rustconf-keynote', title: tabTitle };
  const reply = {
    getState: () => state, refreshSettings: () => state, connect: () => state, start: () => state,
    getTab: () => ({ pageUrl: tab.url, media, attachments: [] }),
    getMedia: () => ({ items: media }),
    setBrowserSettings: (m) => ({ ...state, settings: { ...settings, browser: { ...settings.browser, ...m.patch } } }),
  };
  const noop = () => {};
  const event = { addListener: noop, removeListener: noop };
  globalThis.chrome = {
    runtime: {
      id: 'nhclkmkhoglkcoeiijoonmdmfmgigdhp',
      getManifest: () => ({ version: '2.0.0' }),
      getURL: (p) => `${origin}/extension/${p}`,
      sendMessage: async (msg) => structuredClone(reply[msg?.type]?.(msg) ?? {}),
      onMessage: event,
    },
    storage: { local: { get: async () => ({ settings }), set: async () => {} }, onChanged: event },
    tabs: { query: async () => [tab], get: async () => tab },
    scripting: { executeScript: async () => [{ result: { title: tab.title, items: links } }] },
  };
}

function serve() {
  const server = http.createServer(async (req, res) => {
    try {
      const u = new URL(req.url, 'http://x');
      const file = path.join(ROOT, path.normalize(decodeURIComponent(u.pathname)));
      if (!file.startsWith(ROOT)) { res.writeHead(403).end(); return; }
      if (u.pathname === VIDEO_PAGE) {
        res.writeHead(200, { 'content-type': TYPES['.html'] }).end(videoPage());
        return;
      }
      res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' }).end(await fs.readFile(file));
    } catch {
      res.writeHead(404).end('not found');
    }
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve({ url: `http://127.0.0.1:${server.address().port}`, close: () => server.close() })));
}

// A plain stand-in for a video site; only the extension's button matters here.
const videoPage = () => `<!doctype html><meta charset="utf-8"><title>RustConf 2026 keynote</title>
<style>
  @font-face { font-family: 'Tenor Sans'; src: url('/extension/fonts/tenor-sans-latin.woff2') format('woff2'); }
  @font-face { font-family: 'Archivo Narrow'; src: url('/extension/fonts/archivo-narrow-latin.woff2') format('woff2'); font-weight: 400 700; }
  body { margin: 0; background: #f2f2f0; color: #2b2e33; font: 14px 'Tenor Sans', sans-serif; }
  header { height: 52px; display: flex; align-items: center; padding: 0 28px; background: #fff; border-bottom: 1px solid #e2e2df; font: 700 17px 'Archivo Narrow', sans-serif; }
  main { padding: 24px 28px; }
  video { width: 100%; aspect-ratio: 16/9; display: block; border-radius: 10px; background: #222; }
  h1 { font: 600 22px 'Archivo Narrow', sans-serif; margin: 18px 0 4px; }
  p { margin: 0; color: #6b6f76; }
</style>
<header>talks.example</header>
<main><video src="${VIDEO_SRC}" muted loop playsinline></video>
<h1>RustConf 2026 keynote</h1><p>58 min · 1080p</p></main>`;

async function fontsReady(page, families) {
  await page.waitForFunction((f) => document.fonts.ready.then(() => f.every((x) => document.fonts.check(`13px "${x}"`))), families);
}

const APP_SHOTS = [
  { name: 'app-light', url: '/src/index.html?mock&static&theme=light' },
  { name: 'app-dark', url: '/src/index.html?mock&static&theme=dark' },
  { name: 'app-details', url: '/src/index.html?mock&static&theme=light', prepare: (p) => p.click('.row[data-id="8"] .name'), ready: '.drawer .segs div i' },
  {
    name: 'app-add', url: '/src/index.html?mock&static&ask&theme=light',
    prepare: async (p) => { await p.fill('#paste-input', 'https://cdn.example.com/talks/rustconf-keynote-2026-1080p.mp4'); await p.press('#paste-input', 'Enter'); },
    ready: '.sheet .facts .ok',
  },
  { name: 'app-settings', url: '/src/index.html?mock&static&theme=light', prepare: async (p) => { await p.click('#open-settings'); await p.click('[data-page="browser"]'); }, ready: '.statusbox' },
  { name: 'app-tray', url: '/src/tray.html?mock&static&theme=light', viewport: { width: 320, height: 372 }, ready: '.trow' },
];

async function appShots(server) {
  const browser = await webkit.launch();
  for (const s of APP_SHOTS) {
    const ctx = await browser.newContext({ viewport: s.viewport || { width: 900, height: 620 }, deviceScaleFactor: 2 });
    const page = await ctx.newPage();
    await page.goto(server.url + s.url);
    await fontsReady(page, ['Archivo Narrow', 'Tenor Sans']);
    await page.waitForSelector(s.url.includes('tray') ? '.trow' : '.row');
    if (s.prepare) await s.prepare(page);
    if (s.ready) await page.waitForSelector(s.ready);
    await page.mouse.move(1, (s.viewport?.height || 620) - 1);
    await page.waitForTimeout(300);
    const file = path.join(OUT, `${s.name}.png`);
    // The tray window is transparent around its card; keep just the card.
    if (s.name === 'app-tray') await page.locator('.tray').screenshot({ path: file, omitBackground: true });
    else await page.screenshot({ path: file });
    await ctx.close();
    console.log(`  ${s.name}.png`);
  }
  await browser.close();
}

async function extensionShots(server) {
  const browser = await chromium.launch();
  const stub = { origin: server.url, media: media(server.url), links, tabTitle: 'RustConf 2026 keynote' };

  for (const [name, url, viewport, ready] of [
    ['extension-popup', '/extension/popup/popup.html', { width: 380, height: 560 }, '.prow'],
    ['extension-options', '/extension/options/options.html', { width: 900, height: 620 }, 'main h2'],
  ]) {
    const ctx = await browser.newContext({ viewport, deviceScaleFactor: 2 });
    await ctx.addInitScript(chromeStub, stub);
    const page = await ctx.newPage();
    await page.goto(server.url + url);
    await page.waitForSelector(ready);
    await fontsReady(page, ['Archivo Narrow', 'Tenor Sans']);
    await page.waitForTimeout(300);
    // Chrome sizes the popup to its content; crop to the same box.
    const clip = name === 'extension-popup'
      ? await page.evaluate(() => { const r = document.body.getBoundingClientRect(); return { x: 0, y: 0, width: r.width, height: Math.ceil(document.querySelector('.pfoot').getBoundingClientRect().bottom) }; })
      : undefined;
    await page.screenshot({ path: path.join(OUT, `${name}.png`), clip });
    await ctx.close();
    console.log(`  ${name}.png`);
  }

  // The "Download this video" button and its quality menu over a playing video.
  const ctx = await browser.newContext({ viewport: { width: 900, height: 620 }, deviceScaleFactor: 2 });
  await ctx.addInitScript(chromeStub, stub);
  const page = await ctx.newPage();
  await page.goto(server.url + VIDEO_PAGE);
  await page.addScriptTag({ path: path.join(ROOT, 'extension/content.js') });
  await page.evaluate(() => document.querySelector('video').play());
  await page.waitForFunction(() => document.querySelector('video').currentTime > 0.5);
  await page.evaluate(() => document.querySelector('video').pause());
  await page.mouse.move(400, 250);
  await page.mouse.move(420, 260);
  await page.waitForTimeout(400);
  // The button lives in a closed shadow root, so click its caret by position.
  const box = await page.locator('video').boundingBox();
  await page.mouse.click(box.x + box.width - 26, box.y + 28);
  await page.waitForTimeout(400);
  await fontsReady(page, ['Archivo Narrow', 'Tenor Sans']);
  await page.screenshot({ path: path.join(OUT, 'extension-video.png') });
  await ctx.close();
  console.log('  extension-video.png');
  await browser.close();
}

const server = await serve();
await fs.mkdir(OUT, { recursive: true });
try {
  console.log('App (WebKit):');
  await appShots(server);
  console.log('Extension (Chromium):');
  await extensionShots(server);
} finally {
  server.close();
}
