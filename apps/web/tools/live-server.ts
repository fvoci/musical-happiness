/**
 * Starts the real server for the end-to-end tests that run against it
 * (`playwright.live.config.ts`):
 *
 *   node tools/live-server.ts <port>
 *
 * The server is the one built in this checkout (`cargo build -p
 * fvoci-server`, `target/debug/fvoci-server`; `FVOCI_SERVER_BIN` names
 * another). It runs on SQLite in a new folder under `target/web-live/`, on
 * 127.0.0.1 only, and serves this app's build (`dist/`), so that the page and
 * the API have one origin. It prints `live: <address>` once it answers.
 *
 * When this process is asked to stop, or ends, the server is stopped too.
 * The data folder stays, for looking into after a failure; each run makes a
 * new one.
 */
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const WEB_ROOT = path.resolve(import.meta.dirname, '..');
const REPO_ROOT = path.resolve(WEB_ROOT, '../..');

const port = Number(process.argv[2] ?? 18510);
const binary = process.env.FVOCI_SERVER_BIN ?? path.join(REPO_ROOT, 'target/debug/fvoci-server');
const dist = path.join(WEB_ROOT, 'dist');

function stop(message: string): never {
  console.error(`live-server: ${message}`);
  process.exit(1);
}

if (!existsSync(binary)) stop(`no server at ${binary}: run \`cargo build -p fvoci-server\``);
if (!existsSync(path.join(dist, 'index.html'))) stop('no build in dist/: run `pnpm run build`');

const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const data = path.join(REPO_ROOT, 'target/web-live', `${stamp}-${port}`);
mkdirSync(path.join(data, 'files'), { recursive: true });
const origin = `http://127.0.0.1:${port}`;
writeFileSync(
  path.join(data, 'config.json'),
  JSON.stringify(
    {
      serverRoot: origin,
      port,
      localOnly: true,
      dbtype: 'sqlite3',
      dbconfig: path.join(data, 'fvoci.db'),
      webpath: dist,
      filespath: path.join(data, 'files'),
      // The tests sign in many times from one address.
      login_address_attempts_per_minute: 1000,
    },
    null,
    2,
  ),
);

const server = spawn(binary, ['--config', path.join(data, 'config.json')], {
  cwd: data,
  stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, FVOCI_LOG_FORMAT: 'json', NO_COLOR: '1' },
});

let ready = false;
const watch = (chunk: Buffer): void => {
  const text = chunk.toString();
  process.stderr.write(text);
  if (!ready && text.includes('http server started')) {
    ready = true;
    console.log(`live: ${origin}/ (data in ${path.relative(REPO_ROOT, data)})`);
  }
};
server.stdout.on('data', watch);
server.stderr.on('data', watch);
server.on('exit', (code, signal) => {
  console.error(`live-server: the server ended (${signal ?? code})`);
  process.exit(ready ? 0 : 1);
});

const end = (): void => {
  server.kill('SIGTERM');
  // A server that does not stop within the time it gives its own requests is ended.
  setTimeout(() => server.kill('SIGKILL'), 12_000).unref();
};
process.on('SIGTERM', end);
process.on('SIGINT', end);
process.on('exit', () => server.kill('SIGKILL'));
