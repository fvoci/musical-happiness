/**
 * Proves that the web app stands without the original: copies it to a place
 * that has neither `webapp/` nor `server/`, and runs install, check, test,
 * build and the end-to-end tests there.
 *
 *   node tools/detached/verify.ts <folder> [--port <number>] [--fonts <folder>]
 *                                 [--no-git] [--replace] [--tail <lines>] [--timeout <minutes>]
 *
 * `<folder>` is where the copy goes: `<folder>/copy` gets the files,
 * `<folder>/logs` the whole output of each step. The folder has to be new or
 * empty; `--replace` empties one this tool made before (it leaves a marker
 * in it) and refuses any other.
 *
 * The copy holds what a repository of the web app alone would: the files git
 * tracks under `apps/web`, and the tracked files that lie directly in the
 * root of the repository (`package.json`, the lock file, the workspace file,
 * `deny.toml`, ...). The files are taken from the working tree as they are
 * now. Untracked files are not copied; when there are some, they are listed.
 *
 * Two things are added to the copy:
 *
 * - The fonts. The build's font step writes them and they are not committed;
 *   made anew they cost a large download. They are copied from `--fonts`, by
 *   default from this checkout's `src/styles/fonts`.
 * - A git repository (`git init`, `git add -A`), because a real checkout has
 *   one and tools ask it for the list of files. `--no-git` leaves it out, to
 *   see how the tools fare in a folder that is no checkout.
 *
 * Each step prints its command, its exit code, the lines in which a tool
 * said that it skipped something for want of the original, and the end of
 * its output. The exit code of the tool is 0 only when every step passed.
 * `--port` is the port of the end-to-end tests' preview (FVOCI_E2E_PORT).
 * `pnpm` has to be on the PATH.
 */
import { execFileSync, spawnSync } from 'node:child_process';
import {
  closeSync,
  copyFileSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';

import { argumentAfter, SKIP_PREFIX, WEB_ROOT } from '../original.ts';

const MARKER = 'made-by-detached-verify.json';
const WEB = 'apps/web';
/** What the copy must not have: the two folders of the original. */
const ORIGINAL_FOLDERS = ['webapp', 'server'];

const VALUE_FLAGS = ['--port', '--fonts', '--tail', '--timeout'];
const PLAIN_FLAGS = ['--no-git', '--replace'];

function usage(problem: string): never {
  console.error(problem);
  console.error(
    'usage: node tools/detached/verify.ts <folder> [--port <number>] [--fonts <folder>] [--no-git] [--replace] [--tail <lines>] [--timeout <minutes>]',
  );
  process.exit(2);
}

const positional: string[] = [];
const given = process.argv.slice(2);
for (let at = 0; at < given.length; at += 1) {
  const argument = given[at] ?? '';
  if (VALUE_FLAGS.includes(argument)) at += 1;
  else if (argument.startsWith('--') && !PLAIN_FLAGS.includes(argument)) {
    usage(`unknown argument ${argument}`);
  } else if (!argument.startsWith('--')) positional.push(argument);
}
if (positional.length !== 1) usage('name one folder for the copy');

const numberAfter = (flag: string, fallback: number): number => {
  const value = argumentAfter(flag);
  if (value === undefined) return fallback;
  const number = Number(value);
  if (!Number.isInteger(number) || number <= 0) usage(`${flag} takes a whole number above zero`);
  return number;
};

const folder = path.resolve(positional[0] ?? '');
const copy = path.join(folder, 'copy');
const logs = path.join(folder, 'logs');
const port = numberAfter('--port', Number(process.env.FVOCI_E2E_PORT ?? 18310));
const tail = numberAfter('--tail', 25);
const timeoutMinutes = numberAfter('--timeout', 20);
const fonts = path.resolve(argumentAfter('--fonts') ?? path.join(WEB_ROOT, 'src/styles/fonts'));
const withGit = !process.argv.includes('--no-git');

const git = (cwd: string, ...args: string[]): string =>
  execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });

// 1. What to copy: the tracked files of the web app and of the root.
let source: string;
try {
  source = git(WEB_ROOT, 'rev-parse', '--show-toplevel').trim();
} catch {
  console.error(`${WEB_ROOT} is not in a git checkout: there is no list of tracked files to copy`);
  process.exit(2);
}
if (path.relative(source, WEB_ROOT).split(path.sep).join('/') !== WEB) {
  console.error(`the web app is expected at ${WEB} of the repository, not at ${WEB_ROOT}`);
  process.exit(2);
}
const tracked = git(source, 'ls-files', '-z').split('\0').filter(Boolean);
const files = tracked.filter((file) => file.startsWith(`${WEB}/`) || !file.includes('/'));
const rootFiles = files.filter((file) => !file.includes('/'));
const untracked = git(
  source,
  'ls-files',
  '-z',
  '--others',
  '--exclude-standard',
  '--',
  WEB,
  ...rootFiles,
)
  .split('\0')
  .filter(Boolean);
const changed = git(source, 'status', '--porcelain', '--', WEB, ...rootFiles)
  .split('\n')
  .filter((line) => line !== '' && !line.startsWith('??'));
const commit = git(source, 'rev-parse', '--short', 'HEAD').trim();

// The fonts have to be there before anything is made.
if (!existsSync(path.join(fonts, 'stamp.json'))) {
  console.error(
    `no built fonts at ${fonts} (no stamp.json): build them here first ("pnpm --filter web run fonts") or name a folder with --fonts`,
  );
  process.exit(2);
}

// 2. Where to: a folder that is new, empty, or this tool's own.
if (path.relative(source, folder) === '' || folder.startsWith(path.join(source, WEB) + path.sep)) {
  usage('the folder for the copy cannot be the repository or lie in the web app');
}
if (existsSync(folder) && readdirSync(folder).length > 0) {
  if (!process.argv.includes('--replace')) {
    usage(`${folder} is not empty: name a new folder, or pass --replace for one this tool made`);
  }
  if (!existsSync(path.join(folder, MARKER))) {
    usage(`${folder} was not made by this tool (no ${MARKER} in it): it is not emptied`);
  }
  for (const name of readdirSync(folder)) rmSync(path.join(folder, name), { recursive: true });
}
mkdirSync(copy, { recursive: true });
mkdirSync(logs, { recursive: true });
writeFileSync(
  path.join(folder, MARKER),
  `${JSON.stringify({ from: source, commit, uncommittedChanges: changed.length, withGit }, null, 2)}\n`,
);

for (const file of files) {
  const from = path.join(source, file);
  const to = path.join(copy, file);
  // A tracked file that was deleted in the working tree is not in the copy either.
  const found = lstatSync(from, { throwIfNoEntry: false });
  if (!found) continue;
  mkdirSync(path.dirname(to), { recursive: true });
  if (found.isSymbolicLink()) symlinkSync(readlinkSync(from), to);
  else copyFileSync(from, to);
}

console.log(`copy: ${copy}`);
console.log(
  `copy: ${files.length} tracked files of ${source} at ${commit} (${files.length - rootFiles.length} under ${WEB}, ${rootFiles.length} in the root)`,
);
if (changed.length > 0) {
  console.log(`copy: ${changed.length} of them have changes that are not committed`);
}
if (untracked.length > 0) {
  console.log(
    `copy: NOT copied, because git does not track them:\n  ${untracked.slice(0, 20).join('\n  ')}${
      untracked.length > 20 ? `\n  … and ${untracked.length - 20} more` : ''
    }`,
  );
}
const present = ORIGINAL_FOLDERS.filter((name) => existsSync(path.join(copy, name)));
if (present.length > 0) {
  console.error(`copy: it has ${present.join(' and ')}, which it must not`);
  process.exit(1);
}
console.log(
  `copy: no ${ORIGINAL_FOLDERS.join('/, no ')}/ in it: ${readdirSync(copy).sort().join(' ')}`,
);

// 3. The fonts, which are built and not committed.
cpSync(fonts, path.join(copy, WEB, 'src/styles/fonts'), { recursive: true });
console.log(`copy: fonts from ${fonts}`);

// 4. A repository of its own, as a real checkout has.
if (withGit) {
  git(copy, 'init', '--quiet');
  git(copy, 'add', '--all');
  const staged = git(copy, 'ls-files', '-z').split('\0').filter(Boolean).length;
  console.log(`copy: git init and git add --all (${staged} files in the index)`);
} else {
  console.log('copy: no git repository in it (--no-git)');
}

// 5. The steps.
interface Step {
  name: string;
  args: string[];
  env?: Record<string, string>;
  /** A step that failed makes this one pointless. */
  needs?: string;
}
const STEPS: Step[] = [
  { name: 'install', args: ['install', '--frozen-lockfile'] },
  { name: 'check', args: ['--filter', 'web', 'run', 'check'], needs: 'install' },
  { name: 'test', args: ['--filter', 'web', 'run', 'test'], needs: 'install' },
  { name: 'build', args: ['--filter', 'web', 'run', 'build'], needs: 'install' },
  {
    name: 'e2e',
    args: ['--filter', 'web', 'run', 'e2e'],
    env: { FVOCI_E2E_PORT: String(port) },
    needs: 'build',
  },
];

// Nothing of the caller's environment may lead the copy back to the original,
// or the end-to-end tests to a server that is not the copy's preview.
const environment: NodeJS.ProcessEnv = { ...process.env, NO_COLOR: '1', FORCE_COLOR: '0' };
for (const name of ['FVOCI_ORIGINAL_WEBAPP', 'FVOCI_TEMPLATE_ARCHIVE', 'FVOCI_E2E_URL']) {
  Reflect.deleteProperty(environment, name);
}

// Without a repository of its own, git would answer for an enclosing one.
if (!withGit) environment.GIT_CEILING_DIRECTORIES = path.dirname(copy);

type Result = 'passed' | 'FAILED' | 'not run';
const results: { step: Step; result: Result; code: string; seconds: number; skipped: string[] }[] =
  [];

for (const [at, step] of STEPS.entries()) {
  const command = `${Object.entries(step.env ?? {})
    .map(([name, value]) => `${name}=${value} `)
    .join('')}pnpm ${step.args.join(' ')}`;
  console.log(`\n=== [${at + 1}/${STEPS.length}] ${step.name}: ${command}`);
  const needed = results.find((earlier) => earlier.step.name === step.needs);
  if (needed && needed.result !== 'passed') {
    console.log(`not run: ${needed.step.name} did not pass`);
    results.push({ step, result: 'not run', code: '-', seconds: 0, skipped: [] });
    continue;
  }

  const log = path.join(logs, `${at + 1}-${step.name}.log`);
  const descriptor = openSync(log, 'w');
  const started = performance.now();
  const run = spawnSync('pnpm', step.args, {
    cwd: copy,
    env: { ...environment, ...step.env },
    stdio: ['ignore', descriptor, descriptor],
    timeout: timeoutMinutes * 60_000,
  });
  closeSync(descriptor);
  const seconds = Math.round((performance.now() - started) / 100) / 10;
  const output = readFileSync(log, 'utf8');
  const code = run.error
    ? `could not finish (${run.error.message})`
    : run.signal
      ? `killed by ${run.signal}`
      : String(run.status);
  const skipped = [...new Set(output.split('\n').filter((line) => line.includes(SKIP_PREFIX)))].map(
    (line) => line.trim(),
  );

  console.log(`exit code: ${code} (${seconds} s, whole output in ${log})`);
  for (const line of skipped) console.log(line);
  console.log(`--- the last ${tail} lines`);
  console.log(output.trimEnd().split('\n').slice(-tail).join('\n'));
  console.log('---');
  results.push({ step, result: run.status === 0 ? 'passed' : 'FAILED', code, seconds, skipped });
}

console.log('\n=== summary');
for (const { step, result, code, seconds, skipped } of results) {
  console.log(
    `${step.name.padEnd(8)} ${result.padEnd(8)} exit ${code}  ${seconds} s${
      skipped.length > 0 ? `  (${skipped.length} skipped for want of the original)` : ''
    }`,
  );
}
const failed = results.filter(({ result }) => result !== 'passed');
console.log(
  failed.length === 0
    ? `detached: every step passed in ${copy}, which has no ${ORIGINAL_FOLDERS.join('/ and no ')}/`
    : `detached: ${failed.map(({ step }) => step.name).join(', ')} did not pass`,
);
process.exit(failed.length === 0 ? 0 : 1);
