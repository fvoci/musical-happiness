import { defineConfig, devices } from '@playwright/test';

/**
 * End-to-end tests against the real server: `pnpm --filter web run e2e:live`.
 *
 * tools/live-server.ts starts the server built in this checkout (`cargo
 * build -p fvoci-server`) on SQLite, in a new data folder under
 * `target/web-live/`, serving this app's build (`pnpm run build` first). The
 * server stops when the run ends, passed or failed.
 *
 * `FVOCI_LIVE_PORT` moves it to another port (18510 by default), so that
 * two checkouts can run at once. The tests share the one server: each makes
 * the accounts and boards it needs, under names of its own.
 */
const PORT = Number(process.env.FVOCI_LIVE_PORT ?? 18510);

export default defineConfig({
  testDir: 'e2e-live',
  outputDir: 'test-results/live',
  forbidOnly: Boolean(process.env.CI),
  reporter: [['list']],
  // One server, one database: the tests run one after another.
  workers: 1,
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'desktop', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: `node tools/live-server.ts ${PORT}`,
    // No `url` or `port`: on this host a connection to a closed port hangs
    // instead of being refused (playwright.config.ts says more).
    wait: { stdout: /live: http/ },
    stdout: 'pipe',
    stderr: 'ignore',
    timeout: 60_000,
    gracefulShutdown: { signal: 'SIGTERM', timeout: 15_000 },
  },
});
