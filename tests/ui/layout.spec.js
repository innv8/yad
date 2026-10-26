// Cross-engine layout check: the same screens rendered by Chromium (≈ WebView2) and WebKit
// (≈ WKWebView / WebKitGTK) must put key elements in the same place (±2px).
// Also fails on console errors and on any request that leaves the local server.
import { test, expect, chromium, webkit } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { startServer } from './server.js';

const OUT = path.join(path.dirname(fileURLToPath(import.meta.url)), '.output');
const TOLERANCE = 2;

const SELECTORS = [
  '.titlebar', '.titlebar .brand', '.titlebar .ttl', '.wc button', '.banner',
  'aside.side', '.nav button', '.nav h6', '.svc div',
  '.top', '.paste', '.paste input', '.paste kbd', '#open-settings',
  '.bar', '.bar h2', '.bar .right span', '.group', '.nothing',
  '.row', '.row .tile', '.row .name', '.row .sub', '.row .track', '.row .pct', '.row .pill', '.row .icon-btn',
  '.drawer', '.drawer h3', '.drawer .btn', '.segs div', '.spark', '.kv dt', '.kv dd',
  '.menu', '.menu .mi', '.scrim', '.sheet', '.sheet .input', '.sheet .facts', '.sheet .btn', '.sheet .check',
  'main.settings h2', '.srow', '.toggle', '.select', '.stepper', '.segctl button', '.chips button', '.statusbox',
  '.empty .big', '.empty h2', '.empty p', '.empty .ways div', '.toast',
  '.tray', '.tray .th', '.trow', '.tray .mi',
];

const clickSel = (sel) => async (page) => { await page.click(sel); };
const MIN = { width: 720, height: 520 }; // the window's minimum size
const STATES = [
  { name: 'main-light', url: '/index.html?mock&static&theme=light' },
  { name: 'main-dark', url: '/index.html?mock&static&theme=dark' },
  { name: 'empty', url: '/index.html?mock&static&empty' },
  { name: 'offline', url: '/index.html?mock&static&offline', ready: '.banner:not([hidden])' },
  {
    name: 'details', url: '/index.html?mock&static&theme=dark',
    prepare: clickSel('.row[data-id="8"] .name'), ready: '.drawer .segs div i',
  },
  { name: 'menu', url: '/index.html?mock&static', prepare: clickSel('.row[data-id="4"] [data-act="more"]'), ready: '.menu .mi' },
  {
    name: 'add-sheet', url: '/index.html?mock&static&ask',
    prepare: async (page) => {
      await page.fill('#paste-input', 'https://cdn.example.com/talks/rustconf-keynote-2026-1080p.mp4');
      await page.press('#paste-input', 'Enter');
    },
    ready: '.sheet .facts .ok',
  },
  { name: 'completed', url: '/index.html?mock&static', prepare: clickSel('[data-filter="completed"]') },
  {
    name: 'undo-toast', url: '/index.html?mock&static',
    prepare: async (page) => { await page.click('.row[data-id="6"] .name'); await page.keyboard.press('Escape'); await page.keyboard.press('Delete'); },
    ready: '.toast',
  },
  {
    name: 'filtered-empty', url: '/index.html?mock&static',
    prepare: async (page) => { await page.click('[data-filter="paused"]'); await page.click('.row[data-id="6"] .name'); await page.keyboard.press('Escape'); await page.keyboard.press('Delete'); },
    ready: '.nothing',
  },
  { name: 'settings-general', url: '/index.html?mock&static', prepare: clickSel('#open-settings'), ready: '.srow' },
  { name: 'settings-downloads', url: '/index.html?mock&static&theme=dark', prepare: async (p) => { await p.click('#open-settings'); await p.click('[data-page="downloads"]'); }, ready: '.stepper' },
  { name: 'settings-browser', url: '/index.html?mock&static', prepare: async (p) => { await p.click('#open-settings'); await p.click('[data-page="browser"]'); }, ready: '.statusbox' },
  { name: 'settings-appearance', url: '/index.html?mock&static', prepare: async (p) => { await p.click('#open-settings'); await p.click('[data-page="appearance"]'); }, ready: '.segctl' },
  { name: 'settings-about', url: '/index.html?mock&static', prepare: async (p) => { await p.click('#open-settings'); await p.click('[data-page="about"]'); }, ready: '.about-mark' },
  { name: 'min-main', url: '/index.html?mock&static', viewport: MIN },
  { name: 'min-empty', url: '/index.html?mock&static&empty', viewport: MIN },
  { name: 'min-details', url: '/index.html?mock&static', viewport: MIN, prepare: clickSel('.row[data-id="8"] .name'), ready: '.drawer .segs div i' },
  { name: 'min-settings-browser', url: '/index.html?mock&static', viewport: MIN, prepare: async (p) => { await p.click('#open-settings'); await p.click('[data-page="browser"]'); }, ready: '.statusbox' },
  { name: 'add-to-queue', url: '/index.html?mock&static&ask',
    prepare: async (page) => {
      await page.fill('#paste-input', 'https://example.com/files/big-archive.zip');
      await page.press('#paste-input', 'Enter');
      await page.waitForSelector('.sheet .facts .ok');
      await page.click('.sheet [data-a="queue"]');
    },
    ready: '.row[data-id="9"] [data-act="primary"].solid' },
  { name: 'tray-light', url: '/tray.html?mock&static&theme=light', viewport: { width: 320, height: 372 }, ready: '.trow' },
  { name: 'tray-bottom', url: '/tray.html?mock&static&anchor=bottom', viewport: { width: 320, height: 372 }, ready: 'body.anchor-bottom .trow' },
  { name: 'tray-dark', url: '/tray.html?mock&static&theme=dark', viewport: { width: 320, height: 372 }, ready: '.trow' },
];

let server;
const engines = {};

test.beforeAll(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  server = await startServer();
  // Playwright hides scrollbars in headless Chromium; real WebView2 draws our 10px ::-webkit-scrollbar.
  engines.chromium = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'] });
  engines.webkit = await webkit.launch();
});
test.afterAll(async () => {
  await Promise.all(Object.values(engines).map((b) => b.close()));
  await server?.close();
});

async function capture(engine, st) {
  const ctx = await engines[engine].newContext({
    viewport: st.viewport || { width: 900, height: 620 }, deviceScaleFactor: 1, colorScheme: 'light',
  });
  const page = await ctx.newPage();
  const errors = [];
  const external = [];
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('request', (r) => {
    const u = r.url();
    if (!u.startsWith(server.url) && !u.startsWith('data:') && !u.startsWith('blob:')) external.push(u);
  });
  await page.goto(server.url + st.url);
  await page.waitForFunction(() => document.fonts.status === 'loaded'
    && document.fonts.check('600 13px "Archivo Narrow"') && document.fonts.check('13px "Tenor Sans"'));
  await page.waitForSelector(st.url.includes('tray') ? '.tray' : '.nav button');
  if (!st.url.includes('empty') && !st.url.includes('offline') && !st.url.includes('tray')) await page.waitForSelector('.row');
  if (st.prepare) await st.prepare(page);
  if (st.ready) await page.waitForSelector(st.ready);
  await page.mouse.move(0, (st.viewport || { height: 620 }).height - 1); // no hover states
  await page.waitForTimeout(150);
  await page.evaluate(() => document.fonts.ready);

  const boxes = await page.evaluate((sels) => {
    const out = {};
    for (const sel of sels) {
      document.querySelectorAll(sel).forEach((el, i) => {
        const r = el.getBoundingClientRect();
        if (!r.width || !r.height || !el.getClientRects().length) return;
        out[`${sel} #${i}`] = [r.x, r.y, r.width, r.height].map((v) => Math.round(v * 10) / 10);
      });
    }
    return out;
  }, SELECTORS);
  // Snapshot errors first: in WebKit, Playwright's screenshot helper injects an inline <style>
  // that the test server's strict CSP (rightly) refuses — that report is not the app's.
  const appErrors = [...errors];
  await page.screenshot({ path: path.join(OUT, `${st.name}-${engine}.png`) });
  await ctx.close();
  return { boxes, errors: appErrors, external };
}

for (const st of STATES) {
  test(`same layout in chromium and webkit: ${st.name}`, async () => {
    const c = await capture('chromium', st);
    const w = await capture('webkit', st);
    fs.writeFileSync(path.join(OUT, `${st.name}.boxes.json`), JSON.stringify({ chromium: c.boxes, webkit: w.boxes }, null, 1));

    expect.soft(c.errors, 'chromium console errors').toEqual([]);
    expect.soft(w.errors, 'webkit console errors').toEqual([]);
    expect.soft(c.external, 'chromium external requests').toEqual([]);
    expect.soft(w.external, 'webkit external requests').toEqual([]);

    expect(Object.keys(c.boxes).length, 'captured some elements').toBeGreaterThan(5);
    expect(Object.keys(w.boxes).sort(), 'same elements visible').toEqual(Object.keys(c.boxes).sort());
    const diffs = [];
    for (const [k, a] of Object.entries(c.boxes)) {
      const b = w.boxes[k];
      if (!b) continue;
      const d = Math.max(...a.map((v, i) => Math.abs(v - b[i])));
      if (d > TOLERANCE) diffs.push(`${k}: chromium ${a.join(',')} vs webkit ${b.join(',')}`);
    }
    expect(diffs, 'boxes differ by more than 2px').toEqual([]);
  });
}

test('progress patches rows in place and undo restores a removed row', async () => {
  const ctx = await engines.chromium.newContext({ viewport: { width: 900, height: 620 } });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(`${server.url}/index.html?mock`);
  const row = page.locator('.row[data-id="8"]');
  await row.waitFor();
  await page.evaluate(() => { window.__row = document.querySelector('.row[data-id="8"]'); });
  const before = await row.locator('.sub').textContent();
  await expect.poll(() => row.locator('.sub').textContent(), { timeout: 3000 }).not.toBe(before);
  expect(await page.evaluate(() => window.__row === document.querySelector('.row[data-id="8"]'))).toBe(true);

  await page.click('.row[data-id="6"] .name');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Delete');
  await expect(page.locator('.row[data-id="6"]')).toHaveCount(0);
  await page.click('.toast button');
  await expect(page.locator('.row[data-id="6"]')).toHaveCount(1);
  expect(await page.evaluate(() => window.__YAD_MOCK__.db.has(6))).toBe(true);
  expect(errors).toEqual([]);
  await ctx.close();
});

test('clipboard goes through tauri clipboard-manager (tray read, row "Copy link" write)', async () => {
  const ctx = await engines.chromium.newContext({ viewport: { width: 900, height: 620 } });
  const errors = [];
  const tray = await ctx.newPage();
  tray.on('pageerror', (e) => errors.push(String(e)));
  await tray.goto(`${server.url}/tray.html?mock&static`);
  await tray.waitForSelector('.trow');
  await tray.click('[data-a="clipboard"]');
  await expect.poll(() => tray.evaluate(() => [...window.__YAD_MOCK__.db.values()].some((d) => d.fileName === 'from-clipboard.zip'))).toBe(true);

  const page = await ctx.newPage();
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(`${server.url}/index.html?mock&static`);
  await page.click('.row[data-id="4"] .name', { button: 'right' });
  await page.click('.menu .mi:has-text("Copy link")');
  await expect(page.locator('.toast')).toHaveText('Link copied');
  expect(await page.evaluate(() => window.__TAURI__.clipboardManager.readText())).toContain('productivity-statistics');
  expect(errors).toEqual([]);
  await ctx.close();
});
