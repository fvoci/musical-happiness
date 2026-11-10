import { defineConfig, devices } from '@playwright/test';

/**
 * End-to-end tests.
 *
 * `FVOCI_E2E_URL` is the address of a running server that serves this build
 * (its `webpath` set to `dist/`). Without it the tests start tools/preview.ts,
 * which serves `dist/` with the server's rules but has no API: enough for the
 * tests of the static shell, and only for those.
 */
const external = process.env.FVOCI_E2E_URL;
const PREVIEW_PORT = 18310;

export default defineConfig({
  testDir: 'e2e',
  outputDir: 'test-results',
  forbidOnly: Boolean(process.env.CI),
  reporter: [['list']],
  use: {
    baseURL: external ?? `http://127.0.0.1:${PREVIEW_PORT}`,
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'] } },
    { name: 'mobile', use: { ...devices['Pixel 7'] } },
  ],
  ...(external
    ? {}
    : {
        webServer: {
          command: `node tools/preview.ts ${PREVIEW_PORT}`,
          // No `url` or `port`: with one, Playwright first connects to see
          // whether the address is free, and on a host that drops connections
          // to closed ports instead of refusing them that takes minutes.
          wait: { stdout: /preview: http/ },
          stdout: 'pipe',
          timeout: 20_000,
        },
      }),
});
