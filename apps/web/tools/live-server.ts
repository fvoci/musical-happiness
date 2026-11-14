/**
 * Starts the real server for the end-to-end tests that run against it
 * (`playwright.live.config.ts`):
 *
 *   node tools/live-server.ts <port>
 *
 * The server is the one built in this checkout (`cargo build -p
 * fvoci-server`, `target/debug/fvoci-server`; `FVOCI_SERVER_BIN` names
 * another). It runs on SQLite (or PostgreSQL with `--organizations`) in a new folder under `target/web-live/`, on
 * localhost only, and serves this app's build (`dist/`), so that the page and
 * the API have one origin. It prints `live: <address>` once it answers.
 *
 * Beside the SQLite server runs the fake OpenID Connect provider of the server's tests
 * (`cargo build -p fvoci-server --bins --examples`), on the next port, and
 * the server is started to reach it: with a key to seal its client secret,
 * private addresses allowed, and its certificate as the one it trusts. A second
 * one runs five ports up, for the test that moves the organization from one
 * provider to another (`e2e-live/sso-provider-move.spec.ts`); the server trusts both.
 *
 * Where the data folder of the server on a port is, and its administration
 * socket, is written to `target/web-live/server-<port>.json` (`{ "data": …,
 * "socket": … }`), for a test that must set up what no request can (a second
 * workspace) or act as the server's operator (`e2e-live/support.ts`). The
 * socket is in the system's temporary folder: a socket's path is short.
 *
 * When this process is asked to stop, or ends, the server is stopped too.
 * The data folder stays, with the server's log (`server.log`), for looking
 * into after a failure; each run makes a new one. A server that ends
 * without being asked ends this process with a failure.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';

const WEB_ROOT = path.resolve(import.meta.dirname, '..');
const REPO_ROOT = path.resolve(WEB_ROOT, '../..');

const port = Number(process.argv[2] ?? 18510);
const organizations = process.argv.includes('--organizations');
const binary = process.env.FVOCI_SERVER_BIN ?? path.join(REPO_ROOT, 'target/debug/fvoci-server');
const dist = path.join(WEB_ROOT, 'dist');
const fakeOidc =
  process.env.FVOCI_FAKE_OIDC_BIN ?? path.join(REPO_ROOT, 'target/debug/examples/fake_oidc');
/** The fake provider listens on the port after the server's (`e2e-live/oidc.spec.ts`). */
const oidcPort = port + 1;
/** The provider an organization moves to (`e2e-live/sso-provider-move.spec.ts`): the port the
 * organizations server, four up, would give a provider if it had one. */
const nextOidcPort = port + 5;
/** The fake SMTP and its inbox take the next two (`e2e-live/mail.spec.ts` reads the inbox at +3); the
 * organizations server runs four ports up (`playwright.live.config.ts`), clear of these. */
const smtpPort = port + 2;
const inboxPort = port + 3;
const fakeSmtp = path.join(REPO_ROOT, 'target/debug/examples/fake_smtp');
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
if (!organizations && !existsSync(fakeOidc)) {
  stop(`no fake provider at ${fakeOidc}: run \`cargo build -p fvoci-server --bins --examples\``);
}

const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const data = path.join(REPO_ROOT, 'target/web-live', `${stamp}-${port}`);
mkdirSync(path.join(data, 'files'), { recursive: true });
const socket = path.join(tmpdir(), `fvoci-live-${port}.socket`);
writeFileSync(
  path.join(REPO_ROOT, 'target/web-live', `server-${port}.json`),
  JSON.stringify({ data, socket }),
);
// Holding the helper's stdin keeps its schema alive. EOF removes only that schema.
let databaseEnded: Promise<number | null> = Promise.resolve(0);
let databaseClosing = false;
const database = organizations
  ? spawn(path.join(REPO_ROOT, 'target/debug/examples/live_database'), [data], {
      // Playwright terminates the server process group; stdin owns this helper instead.
      detached: true,
      stdio: ['pipe', 'pipe', 'inherit'],
      env: { ...process.env, FVOCI_STORE_TEST_DB_TYPE: 'postgres' },
    })
  : undefined;
if (database) {
  databaseEnded = new Promise((resolve) => {
    database.on('exit', (code) => {
      resolve(code);
    });
  });
  await new Promise<void>((resolve, reject) => {
    const lines = createInterface({ input: database.stdout });
    lines.on('line', (line) => {
      if (line === 'database: ready') resolve();
    });
    database.on('error', reject);
    database.on('exit', (code) => {
      reject(new Error(`database helper ended (${code})`));
    });
  });
}
const storage = organizations
  ? {
      dbtype: 'postgres',
      ...(JSON.parse(readFileSync(path.join(data, 'database.json'), 'utf8')) as {
        dbconfig: string;
      }),
      org_creation: 'open',
    }
  : { dbtype: 'sqlite3', dbconfig: path.join(data, 'fvoci.db') };
const origin = `http://localhost:${port}`;
writeFileSync(
  path.join(data, 'config.json'),
  JSON.stringify(
    {
      serverRoot: origin,
      smtp_host: '127.0.0.1',
      smtp_port: smtpPort,
      smtp_tls: 'none',
      smtp_from: 'sender@example.test',
      port,
      localOnly: true,
      ...storage,
      webpath: dist,
      filespath: path.join(data, 'files'),
      // Every test signs in and registers from one address, several at a time
      // when the runner has more than one worker: no limit by address, as for a
      // server whose users all come from one.
      login_address_attempts_per_minute: 0,
      // The smallest part the server allows, so that a test file of a few
      // MiB goes up in more than one part.
      upload_part_bytes: 5 * 1024 * 1024,
      // A board shared by a read token, to see what an anonymous reader gets.
      enablepublicsharedboards: true,
      // The operator's commands (`e2e-live/sso-operator.spec.ts`).
      enablelocalmode: true,
      localmodesocketlocation: socket,
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
/** Starts a fake provider on `onPort`, which writes its certificate to `certificate`. */
async function startProvider(onPort: number, certificate: string): Promise<ChildProcess> {
  const provider = spawn(fakeOidc, [String(onPort), certificate], {
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
      if (!stopping) stop(`the fake provider on ${onPort} ended (${code})`);
    });
  });
  return provider;
}

/** The certificates of both providers in one file: what the server trusts. */
const certificates = path.join(data, 'fake-oidc.pem');
const providers: ChildProcess[] = [];
if (!organizations) {
  const own = [path.join(data, 'fake-oidc-1.pem'), path.join(data, 'fake-oidc-2.pem')] as const;
  providers.push(await startProvider(oidcPort, own[0]));
  providers.push(await startProvider(nextOidcPort, own[1]));
  writeFileSync(certificates, own.map((file) => readFileSync(file, 'utf8')).join('\n'));
}

const smtp = spawn(fakeSmtp, [String(smtpPort), String(inboxPort)], {
  cwd: data,
  stdio: ['ignore', 'pipe', 'inherit'],
});
await new Promise<void>((started) => {
  const said = createInterface({ input: smtp.stdout });
  said.on('line', (line) => {
    log.write(`${line}\n`);
    if (line.startsWith('fake-smtp:')) started();
  });
  smtp.on('error', (error) => stop(`the fake SMTP could not start: ${error.message}`));
  smtp.on('exit', (code) => {
    if (!stopping) stop(`the fake SMTP ended (${code})`);
  });
});

const server = spawn(binary, ['--config', path.join(data, 'config.json')], {
  cwd: data,
  stdio: ['ignore', 'pipe', 'pipe'],
  env: {
    ...environment,
    FVOCI_LOG_FORMAT: 'json',
    NO_COLOR: '1',
    ...(!organizations
      ? {
          FVOCI_SECRET_KEY: SECRET_KEY,
          FVOCI_OIDC_ALLOW_PRIVATE_ADDRESSES: 'true',
          // The fake providers' certificates are the ones the server trusts.
          SSL_CERT_FILE: certificates,
        }
      : {}),
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
async function finished(code: number | null, signal: NodeJS.Signals | null): Promise<void> {
  databaseClosing = true;
  database?.stdin.end();
  const databaseCode = await databaseEnded;
  if (databaseCode !== 0) stop(`database cleanup failed (${databaseCode})`);
  log.end();
  // On stdout too: the test runner shows this process's stdout.
  const said = `live-server: the server ended (${signal ?? code})`;
  if (stopping) console.log(said);
  else stop(`${said} unasked; its log is in ${path.relative(REPO_ROOT, data)}/server.log`);
  process.exit(0);
}
server.on('exit', (code, signal) => {
  void finished(code, signal);
});
database?.on('exit', (code) => {
  if (!databaseClosing) stop(`database helper ended unexpectedly (${code})`);
});

const end = (): void => {
  stopping = true;
  for (const provider of providers) provider.kill('SIGTERM');
  smtp.kill('SIGTERM');
  server.kill('SIGTERM');
  // A server that does not stop within the time it gives its own requests is ended.
  setTimeout(() => server.kill('SIGKILL'), 12_000).unref();
};
process.on('SIGTERM', end);
process.on('SIGINT', end);
process.on('exit', () => {
  server.kill('SIGKILL');
  for (const provider of providers) provider.kill('SIGKILL');
  smtp.kill('SIGKILL');
});
