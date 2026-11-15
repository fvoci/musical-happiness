import { defineConfig, devices } from '@playwright/test';

import base from './playwright.config.ts';

/** Apple engines run separately so the default Chromium suite keeps its cost. */
export default defineConfig({
  ...base,
  outputDir: 'test-results/apple',
  workers: 2,
  // Names describe the three layout stages used by the shared browser specs.
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Safari'], deviceScaleFactor: 2 } },
    { name: 'mobile', use: { ...devices['iPhone 17e'] } },
    { name: 'middle', use: { ...devices['iPad Pro 11'] } },
  ],
});
