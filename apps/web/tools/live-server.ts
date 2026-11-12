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
 * Beside it runs the fake OpenID Connect provider of the server's tests
 * (`cargo build -p fvoci-server --bins --examples`), on the next port, and
 * the server is started to reach it: with a key to seal its client secret,
 * private addresses allowed, and its certificate as the one it trusts.
 *
 * Where the data folder of the server on a port is, is written to
 * `target/web-live/server-<port>.json` (`{ "data": … }`), for a test that
 * must set up what no request can (a second workspace, `e2e-live/support.ts`).
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
const fakeOidc =
  process.env.FVOCI_FAKE_OIDC_BIN ?? path.join(REPO_ROOT, 'target/debug/examples/fake_oidc');
/** The fake provider listens on the port after the server's (`e2e-live/oidc.spec.ts`). */
const oidcPort = port + 1;
/** The key the server seals client secrets with: a test's, kept nowhere else. */
const SECRET_KEY = Buffer.alloc(32, 7).toString('base64');

/** Ends with a message where the test runner shows it (stdout, and stderr). */
function stop(message: string): never {
  console.log(`live-server: ${message}`);
  console.error(`live-server: ${message}`);
  process.exit(1);
}

if (!existsSync(binary)) stop(`no server at ${binary}: run \`cargo build -p fvoci-server\``);
if (!existsSync(path.join(dist, 'index.html'))) stop('no build in dist/: run `pnpm run build`');
if (!existsSync(fakeOidc)) {
  stop(`no fake provider at ${fakeOidc}: run \`cargo build -p fvoci-server --bins --examples\``);
}

const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const data = path.join(REPO_ROOT, 'target/web-live', `${stamp}-${port}`);
mkdirSync(path.join(data, 'files'), { recursive: true });
writeFileSync(
  path.join(REPO_ROOT, 'target/web-live', `server-${port}.json`),
  JSON.stringify({ data }),
);
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
      // The tests sign in and make accounts from one address, several at
      // once: the limit by address, which lets one attempt of an address go
      // at a time, is off. The server's own tests are about it.
      login_address_attempts_per_minute: 0,
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

/** Whether this process is stopping, and its server and provider with it. */
let stopping = false;
const certificate = path.join(data, 'fake-oidc.pem');
const provider = spawn(fakeOidc, [String(oidcPort), certificate], {
  cwd: data,
  stdio: ['ignore', 'pipe', 'inherit'],
});
await new Promise<void>((started) => {
  const said = createInterface({ input: provider.stdout });
  said.on('line', (line) => {
    log.write(`${line}\n`);
    if (line.startsWith('fake-oidc:')) started();
  });
  provider.on('exit', (code) => {
    if (!stopping) stop(`the fake provider ended (${code})`);
  });
});

const server = spawn(binary, ['--config', path.join(data, 'config.json')], {
  cwd: data,
  stdio: ['ignore', 'pipe', 'pipe'],
  env: {
    ...environment,
    FVOCI_LOG_FORMAT: 'json',
    NO_COLOR: '1',
    FVOCI_SECRET_KEY: SECRET_KEY,
    FVOCI_OIDC_ALLOW_PRIVATE_ADDRESSES: 'true',
    // The fake provider's certificate is the one the server trusts.
    SSL_CERT_FILE: certificate,
  },
});

let ready = false;
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
  provider.kill('SIGTERM');
  server.kill('SIGTERM');
  // A server that does not stop within the time it gives its own requests is ended.
  setTimeout(() => server.kill('SIGKILL'), 12_000).unref();
};
process.on('SIGTERM', end);
process.on('SIGINT', end);
process.on('exit', () => {
  server.kill('SIGKILL');
  provider.kill('SIGKILL');
});
