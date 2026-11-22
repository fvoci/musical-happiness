import { defineConfig, devices } from '@playwright/test';

/**
 * Browser tests of the spikes (`spikes/<name>/*.spec.ts`), against the
 * spikes' development server. Separate from the product's end-to-end tests.
 *
 *   pnpm run spikes:e2e
 */
const PORT = Number(process.env.FVOCI_SPIKES_PORT ?? 18320);
/** The spikes measured on a phone's screen only. */
const PHONE_ONLY = /spikes[\\/]kanban-scroll[\\/][^\\/]+\.spec\.ts$/;
/** An iPhone's screen of 390 by 844 points. */
const PHONE = {
  viewport: { width: 390, height: 844 },
  deviceScaleFactor: 3,
  isMobile: true,
  hasTouch: true,
};

/** The remote-update spike: measurements, not checks, kept out of the other projects. */
const REMOTE = /spikes[\\/]remote[\\/].*\.spec\.ts$/;

export default defineConfig({
  testDir: 'spikes',
  outputDir: 'test-results/spikes',
  forbidOnly: Boolean(process.env.CI),
  reporter: [['list']],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', testIgnore: [PHONE_ONLY, REMOTE], use: { ...devices['Desktop Chrome'] } },
    { name: 'touch', testIgnore: [PHONE_ONLY, REMOTE], use: { ...devices['Pixel 7'] } },
    // A phone's screen in both engines, for what is measured on one (spikes/kanban-scroll),
    // only when there is a built app to measure: a plain run has no WebKit to start.
    ...(process.env.FVOCI_SPIKE_APP
      ? [
          {
            name: 'phone-webkit',
            testMatch: PHONE_ONLY,
            use: { ...PHONE, browserName: 'webkit' as const },
          },
          {
            name: 'phone-chromium',
            testMatch: PHONE_ONLY,
            use: { ...PHONE, browserName: 'chromium' as const },
          },
        ]
      : []),
    // Measurements of one large body, each run in a browser of its own.
    { name: 'remote-chromium', testMatch: REMOTE, use: { ...devices['Desktop Chrome'] } },
    { name: 'remote-webkit', testMatch: REMOTE, use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: `vite --config vite.spikes.config.ts --port ${PORT}`,
    // No `url` or `port`: see playwright.config.ts.
    wait: { stdout: /Local:/ },
    stdout: 'pipe',
    timeout: 30_000,
  },
});
