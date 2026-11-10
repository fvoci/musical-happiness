/**
 * Fails when a file of the web app contains one of the two names the product
 * was known by before the port. Case does not matter.
 *
 *   node tools/names/check.ts
 *
 * Checked are the files git tracks or would track under apps/web. The names
 * are put together here from halves so that this file passes its own check.
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

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');
const NAMES = [
  ['focal', 'board'],
  ['matter', 'most'],
].map((halves) => halves.join(''));
const pattern = new RegExp(NAMES.join('|'), 'i');

/** The files git tracks or would track; none when git cannot say. */
function filesOfGit(): string[] {
  try {
    return execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard'], {
      cwd: WEB_ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
      .split('\n')
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

const hits: string[] = [];
for (const file of files) {
  if (pattern.test(file)) hits.push(`${file}: in the file name`);
  let text: string;
  try {
    text = readFileSync(path.join(WEB_ROOT, file), 'utf8');
  } catch {
    continue;
  }
  text.split('\n').forEach((line, index) => {
    if (pattern.test(line)) hits.push(`${file}:${index + 1}: ${line.trim().slice(0, 120)}`);
  });
}

if (hits.length > 0) {
  console.error(`old product names found:\n  ${hits.join('\n  ')}`);
  process.exit(1);
}
console.log(`names: ok (${files.length} files)`);
