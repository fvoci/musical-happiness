/**
 * Fails when a file of the web app has the former name of the workspace where
 * the repository's list does not let it stay (`workspace.ts`), or an entry of
 * that list under `apps/web/` is one nothing needs.
 *
 *   node tools/names/check.ts
 *
 * Checked are the files git tracks or would track under apps/web.
 *
 * Where git has no list to give (the folder is not in a repository, as in an
 * unpacked archive, or it lies in an ignored folder of another repository),
 * the folder is walked instead, and a line says so. The walk leaves out what
 * the `.gitignore` files under the web app name, as far as their patterns
 * are plain names, and `node_modules`.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { EXCEPTIONS, checkFile, parseExceptions, staleExceptions } from './workspace.ts';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');

/**
 * The files git tracks or would track; none when git cannot say. The paths are
 * asked for as they are written, ended by a NUL: listed by lines, git quotes a
 * path that has a letter outside ASCII, and the quoted path names no file.
 */
function filesOfGit(): string[] {
  try {
    return execFileSync(
      'git',
      [
        '-c',
        'core.quotePath=false',
        'ls-files',
        '-z',
        '--cached',
        '--others',
        '--exclude-standard',
      ],
      { cwd: WEB_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] },
    )
      .split('\0')
      .filter((file) => file !== '');
  } catch {
    return [];
  }
}

/** What a `.gitignore` leaves out of its folder: everything, or the entries it names. */
function ignoredIn(directory: string): { all: boolean; paths: Set<string> } {
  const file = path.join(directory, '.gitignore');
  const lines = existsSync(file)
    ? readFileSync(file, 'utf8')
        .split('\n')
        .map((line) => line.trim())
        .filter((line) => line !== '' && !line.startsWith('#'))
    : [];
  return {
    all: lines.includes('*'),
    paths: new Set(lines.map((line) => line.replace(/^\/|\/$/g, ''))),
  };
}

/** The files under the web app, found by walking it. Paths relative to it, sorted. */
function filesOfFolder(): string[] {
  const files: string[] = [];
  const walk = (relative: string, ignored: ReadonlySet<string>): void => {
    const directory = path.join(WEB_ROOT, relative);
    const own = ignoredIn(directory);
    if (own.all) return;
    // A pattern with a slash is a path from the folder of its file.
    const here = new Set([...ignored, ...[...own.paths].map((p) => path.join(relative, p))]);
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(relative, entry.name);
      if (entry.name === 'node_modules' || entry.name === '.git') continue;
      if (here.has(file) || own.paths.has(entry.name)) continue;
      if (entry.isDirectory()) walk(file, here);
      else if (entry.isFile()) files.push(file.split(path.sep).join('/'));
    }
  };
  walk('', new Set());
  return files.sort();
}

let files = filesOfGit();
if (files.length === 0) {
  files = filesOfFolder();
  console.log('names: git lists no files here; the folder was walked instead');
}
if (files.length === 0) {
  console.error('names: no files to check');
  process.exit(1);
}

const listed = path.resolve(WEB_ROOT, '../..', EXCEPTIONS);
const exceptions = existsSync(listed)
  ? parseExceptions(readFileSync(listed, 'utf8'))
      .filter((entry) => entry.pattern.startsWith('apps/web/'))
      .map((entry) => ({ ...entry, pattern: entry.pattern.slice('apps/web/'.length) }))
  : [];
const used = exceptions.map(() => false);
const workspaceHits: string[] = [];
for (const file of files) {
  let text = '';
  try {
    text = readFileSync(path.join(WEB_ROOT, file), 'utf8');
  } catch {
    // Not there to read: its name is still checked.
  }
  workspaceHits.push(...checkFile(file, text, exceptions, used));
}
const stale = staleExceptions(exceptions, used);

if (workspaceHits.length > 0 || stale.length > 0) {
  console.error(
    `the former name of the workspace, where ${EXCEPTIONS} does not list it with its reason:\n  ${[...workspaceHits, ...stale].join('\n  ')}`,
  );
  process.exit(1);
}
console.log(`names: ok (${files.length} files, ${exceptions.length} exceptions)`);
