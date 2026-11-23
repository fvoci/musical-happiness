/**
 * Writes the list of what is over the limits of length anew, from the code
 * as it is. See `lengths.ts`.
 *
 *   node tools/lint/measure-lengths.ts     (`pnpm run lengths:measure`)
 *
 * Used to tighten ceilings and drop unneeded entries, or once after work
 * that was measured apart is merged; no check calls it. A ceiling that goes
 * up with its code is one line changed by hand, with evidence (see `lengths.ts`).
 */
import path from 'node:path';

import { EXCEPTIONS, rewrite } from './lengths.ts';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');

try {
  const { list, raised } = await rewrite(WEB_ROOT);
  for (const line of raised) console.log(line);
  const lines = list.split('\n');
  const count = (kind: string): number => lines.filter((line) => line.startsWith(kind)).length;
  console.log(
    `wrote ${EXCEPTIONS}: ${count('file ')} files and ${count('function ')} functions over their limit`,
  );
} catch (error) {
  console.error(`lengths: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}
