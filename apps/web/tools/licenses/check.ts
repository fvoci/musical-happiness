/**
 * Checks the licence of every installed package, tools included.
 *
 *   node tools/licenses/check.ts
 *
 * A licence has to be on the list in the repository's `deny.toml`, or the
 * package has to be listed under `tooling` in exceptions.json with that
 * licence and a reason. An exception that matches nothing is an error too, so
 * the list cannot outlive what it was written for. The build applies the same
 * list to what ships, without the exceptions (notices.ts).
 */
import { execFileSync } from 'node:child_process';
import path from 'node:path';

import exceptions from './exceptions.json' with { type: 'json' };
import { isAllowed, readAllowedLicenses } from './policy.ts';

interface ListedPackage {
  name: string;
  versions: string[];
  license: string;
}

function matchesName(pattern: string, name: string): boolean {
  return pattern.endsWith('*') ? name.startsWith(pattern.slice(0, -1)) : pattern === name;
}

function installedPackages(): Record<string, ListedPackage[]> {
  const pnpm = process.env.npm_execpath ?? 'pnpm';
  const output = execFileSync(pnpm, ['licenses', 'list', '--json'], {
    cwd: path.resolve(import.meta.dirname, '../..'),
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  });
  return JSON.parse(output) as Record<string, ListedPackage[]>;
}

const allowed = new Set(readAllowedLicenses());
const used = new Set<number>();
const refused: string[] = [];
const counts: [string, number, string][] = [];

for (const [license, packages] of Object.entries(installedPackages())) {
  if (isAllowed(license, allowed)) {
    counts.push([license, packages.length, 'allowed']);
    continue;
  }
  let excepted = 0;
  for (const item of packages) {
    const index = exceptions.tooling.findIndex(
      (entry) => entry.license === license && matchesName(entry.name, item.name),
    );
    if (index === -1) refused.push(`${item.name} ${item.versions.join(', ')}: ${license}`);
    else {
      used.add(index);
      excepted += 1;
    }
  }
  counts.push([license, packages.length, `${excepted} excepted as tooling`]);
}

const stale = exceptions.tooling.filter((_, index) => !used.has(index));

for (const [license, count, verdict] of counts.sort((a, b) => b[1] - a[1])) {
  console.log(`${String(count).padStart(5)}  ${license.padEnd(36)} ${verdict}`);
}
if (refused.length > 0) {
  console.error(`\nlicences not allowed by deny.toml and not excepted:\n  ${refused.join('\n  ')}`);
}
if (stale.length > 0) {
  console.error(
    `\nexceptions that match no installed package (remove them):\n  ${stale
      .map((entry) => `${entry.name}: ${entry.license}`)
      .join('\n  ')}`,
  );
}
if (refused.length > 0 || stale.length > 0) process.exit(1);
console.log('\nlicences: ok');
