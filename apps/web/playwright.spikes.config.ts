import { defineConfig, devices } from '@playwright/test';

/**
 * Browser tests of the spikes (`spikes/<name>/*.spec.ts`), against the
 * spikes' development server. Separate from the product's end-to-end tests.
 *
 *   pnpm run spikes:e2e
 */
const PORT = Number(process.env.FVOCI_SPIKES_PORT ?? 18320);

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
    { name: 'desktop', testIgnore: REMOTE, use: { ...devices['Desktop Chrome'] } },
    { name: 'touch', testIgnore: REMOTE, use: { ...devices['Pixel 7'] } },
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
