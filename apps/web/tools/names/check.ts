/**
 * Fails when a file of the web app contains one of the two names the product
 * was known by before the port. Case does not matter.
 *
 *   node tools/names/check.ts
 *
 * Checked are the files git tracks or would track under apps/web. The names
 * are put together here from halves so that this file passes its own check.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');
const NAMES = [
  ['focal', 'board'],
  ['matter', 'most'],
].map((halves) => halves.join(''));
const pattern = new RegExp(NAMES.join('|'), 'i');

const files = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard'], {
  cwd: WEB_ROOT,
  encoding: 'utf8',
})
  .split('\n')
  .filter((file) => file !== '');

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
