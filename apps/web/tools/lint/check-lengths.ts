/**
 * Fails when a file or a function of `src` is longer than its limit, or
 * than the ceiling the list of exceptions gives it. See `lengths.ts`.
 *
 *   node tools/lint/check-lengths.ts            the check, part of `pnpm run check`
 *   node tools/lint/check-lengths.ts --measure  writes the list anew from the code
 *
 * `--measure` is the one way a ceiling is to go up. It is for the moment
 * after work that was measured apart is merged; no check calls it.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import {
  EXCEPTIONS,
  MAX_FILE_LINES,
  MAX_FUNCTION_LINES,
  judge,
  measure,
  renderExceptions,
} from './lengths.ts';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');
const list = path.join(WEB_ROOT, EXCEPTIONS);

const [mode, ...rest] = process.argv.slice(2);
if (rest.length > 0 || (mode !== undefined && mode !== '--measure')) {
  console.error('usage: node tools/lint/check-lengths.ts [--measure]');
  process.exit(2);
}

const measured = await measure(WEB_ROOT);

if (mode === '--measure') {
  const text = renderExceptions(measured);
  writeFileSync(list, text);
  const lines = text.split('\n');
  const count = (kind: string): number => lines.filter((line) => line.startsWith(kind)).length;
  console.log(
    `wrote ${EXCEPTIONS}: ${count('file ')} files and ${count('function ')} functions over their limit`,
  );
} else {
  const problems = judge(measured, readFileSync(list, 'utf8'));
  if (problems.length > 0) {
    for (const problem of problems) console.error(problem);
    console.error(
      `lengths: ${problems.length} differences between the code and ${EXCEPTIONS}. ` +
        'By hand a ceiling goes down and never up: shorten the code, or split it.',
    );
    process.exit(1);
  }
  const files = measured.filter((length) => length.kind === 'file').length;
  console.log(
    `lengths: ${files} files within ${MAX_FILE_LINES} lines and ${measured.length - files} functions ` +
      `within ${MAX_FUNCTION_LINES}, or at their listed ceiling`,
  );
}
