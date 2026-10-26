import { defineConfig } from '@playwright/test';

// The layout spec launches chromium AND webkit itself (playwright.chromium / playwright.webkit)
// so it can compare their boxes inside one test; a single project is enough here.
export default defineConfig({
  testDir: '.',
  testMatch: /.*\.spec\.js$/,
  timeout: 90_000,
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  outputDir: './.output/results',
});
