/**
 * Runs the unit tests once per time zone.
 *
 *   node tools/test-zones.ts [arguments for vitest]
 *
 * Dates are where code quietly depends on the machine it runs on: a test
 * that passes in Seoul can fail in Kiritimati. The three zones are UTC
 * itself, a zone east of it, and the one furthest east (UTC+14), where a
 * local day lies almost entirely on the previous UTC day.
 *
 * Where the original is not there, the tests that convert it again are
 * skipped (tools/original.ts), and a line says which.
 */
import { spawnSync } from 'node:child_process';
import path from 'node:path';

import { skippedTests } from './original.ts';

const ZONES = ['UTC', 'Asia/Seoul', 'Pacific/Kiritimati'];
const vitest = path.resolve(import.meta.dirname, '../node_modules/vitest/vitest.mjs');

// Tests that read the original again are skipped where it is not there. Said
// here, before and after, because a reporter may not show what a test prints.
const skipped = skippedTests();
for (const notice of skipped) console.log(notice);

const failed: string[] = [];
for (const zone of ZONES) {
  console.log(`\n=== TZ=${zone} ===`);
  const result = spawnSync(process.execPath, [vitest, 'run', ...process.argv.slice(2)], {
    cwd: path.resolve(import.meta.dirname, '..'),
    env: { ...process.env, TZ: zone },
    stdio: 'inherit',
  });
  if (result.status !== 0) failed.push(zone);
}

if (failed.length > 0) {
  console.error(`\ntests failed under: ${failed.join(', ')}`);
  process.exit(1);
}
console.log(`\ntests passed under: ${ZONES.join(', ')}`);
for (const notice of skipped) console.log(notice);
