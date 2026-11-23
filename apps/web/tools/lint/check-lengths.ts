/**
 * Fails when a file or a function of `src` is longer than its limit, or
 * than the ceiling the list of exceptions gives it. See `lengths.ts`.
 *
 *   node tools/lint/check-lengths.ts
 *
 * Part of `pnpm run check`. It takes no argument and writes nothing: the
 * list is rewritten by `measure-lengths.ts`, which no check calls.
 */
import path from 'node:path';

import { EXCEPTIONS, MAX_FILE_LINES, MAX_FUNCTION_LINES, check } from './lengths.ts';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');

if (process.argv.length > 2) {
  console.error('usage: node tools/lint/check-lengths.ts (no arguments)');
  process.exit(2);
}

try {
  const { measured, problems } = await check(WEB_ROOT);
  if (problems.length > 0) {
    for (const problem of problems) console.error(problem);
    console.error(
      `lengths: ${problems.length} differences between the code and ${EXCEPTIONS}. ` +
        'Over a limit or ceiling: shorten the code or split it; where a split would make it ' +
        'worse, raise or add its line by hand to the length above, with the evidence in the ' +
        'commit body (see tools/lint/lengths.ts). Remove lines whose file or function is gone. ' +
        '`pnpm run lengths:measure` is only for tightening the list or once after a merge, ' +
        'never to make a check pass.',
    );
    process.exit(1);
  }
  const files = measured.filter((length) => length.kind === 'file').length;
  console.log(
    `lengths: ${files} files within ${MAX_FILE_LINES} lines and ${measured.length - files} functions ` +
      `within ${MAX_FUNCTION_LINES}, or within their listed ceiling`,
  );
} catch (error) {
  console.error(`lengths: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}
