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
 * The data folder stays, with the server's log (`server.log`), for looking
 * into after a failure; each run makes a new one. A server that ends
 * without being asked ends this process with a failure.
 */
import { spawn } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { createInterface } from 'node:readline';

const WEB_ROOT = path.resolve(import.meta.dirname, '..');
const REPO_ROOT = path.resolve(WEB_ROOT, '../..');

const port = Number(process.argv[2] ?? 18510);
const binary = process.env.FVOCI_SERVER_BIN ?? path.join(REPO_ROOT, 'target/debug/fvoci-server');
const dist = path.join(WEB_ROOT, 'dist');

/** Ends with a message where the test runner shows it (stdout, and stderr). */
function stop(message: string): never {
  console.log(`live-server: ${message}`);
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
      // The smallest part the server allows, so that a test file of a few
      // MiB goes up in more than one part.
      upload_part_bytes: 5 * 1024 * 1024,
      // A board shared by a read token, to see what an anonymous reader gets.
      enablepublicsharedboards: true,
    },
    null,
    2,
  ),
);

// The server reads `FVOCI_*` variables over its config file; one left in
// the shell from other work (a database, a port) would move the tests off
// their own SQLite. Only the format of the log is given.
const environment = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.startsWith('FVOCI_')),
);
const log = createWriteStream(path.join(data, 'server.log'));
const server = spawn(binary, ['--config', path.join(data, 'config.json')], {
  cwd: data,
  stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...environment, FVOCI_LOG_FORMAT: 'json', NO_COLOR: '1' },
});

let ready = false;
let stopping = false;
const lines = createInterface({ input: server.stdout });
const errorLines = createInterface({ input: server.stderr });
const watch = (line: string): void => {
  log.write(`${line}\n`);
  if (!ready && line.includes('http server started')) {
    ready = true;
    console.log(`live: ${origin}/ (data and server.log in ${path.relative(REPO_ROOT, data)})`);
  }
};
lines.on('line', watch);
errorLines.on('line', watch);
server.on('exit', (code, signal) => {
  log.end();
  // On stdout too: the test runner shows this process's stdout.
  const said = `live-server: the server ended (${signal ?? code})`;
  if (stopping) console.log(said);
  else stop(`${said} unasked; its log is in ${path.relative(REPO_ROOT, data)}/server.log`);
  process.exit(0);
});

const end = (): void => {
  stopping = true;
  server.kill('SIGTERM');
  // A server that does not stop within the time it gives its own requests is ended.
  setTimeout(() => server.kill('SIGKILL'), 12_000).unref();
};
process.on('SIGTERM', end);
process.on('SIGINT', end);
process.on('exit', () => server.kill('SIGKILL'));
