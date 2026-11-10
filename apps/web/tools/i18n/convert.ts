/**
 * Writes `messages/<locale>.json` from the original translations.
 *
 *   node tools/i18n/convert.ts            write the message files
 *   node tools/i18n/convert.ts --check    fail when they are not what would be written
 *   node tools/i18n/convert.ts --report   print everything the conversion found, as JSON
 *
 * The message files are generated. What is edited by hand is the sources:
 * `messages-own/` (messages only this app has), `messages-fill/`
 * (translations the original lacks), `messages-override/` (texts that replace
 * one of the original) and `tools/i18n/removed.json`. See conversion.ts, and
 * docs/i18n-conversion.md for what was found.
 *
 * `project.inlang/settings.json` is not written, but its locales have to be
 * the ones of locales.ts.
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { convertAll } from './conversion.ts';
import { LOCALES, WEB_ROOT } from './locales.ts';

const mode = process.argv[2];
if (mode !== undefined && mode !== '--check' && mode !== '--report') {
  console.error(`unknown argument ${mode}`);
  process.exit(2);
}

const { files, report } = convertAll();

if (mode === '--report') {
  console.log(JSON.stringify(report, null, 2));
  process.exit(0);
}

const settings = JSON.parse(
  readFileSync(path.join(WEB_ROOT, 'project.inlang/settings.json'), 'utf8'),
) as { locales: string[] };
const tags = LOCALES.map((locale) => locale.tag);
if (settings.locales.join() !== tags.join()) {
  console.error(`project.inlang/settings.json: "locales" has to be ${JSON.stringify(tags)}`);
  process.exit(1);
}

if (mode === '--check') {
  const stale = [...files].filter(([name, content]) => {
    const file = path.join(WEB_ROOT, name);
    return !existsSync(file) || readFileSync(file, 'utf8') !== content;
  });
  if (stale.length > 0) {
    console.error(
      `generated message files differ from their sources:\n  ${stale.map(([name]) => name).join('\n  ')}\n` +
        'They are written by tools/i18n/convert.ts: edit messages-own/, messages-fill/ or messages-override/, then run "pnpm run i18n:convert".',
    );
    process.exit(1);
  }
  console.log(`message files: up to date (${files.size} locales)`);
  process.exit(0);
}

for (const [name, content] of files) writeFileSync(path.join(WEB_ROOT, name), content);

console.log(
  `English: ${report.kept.length} ids kept, ${report.removed.length} removed, ${report.dead.length} dead\n`,
);
console.log(
  'locale   original converted (overridden)  not conv.  stale  removed  dead  filled  own  missing',
);
for (const locale of report.locales) {
  const cells = [
    locale.original,
    locale.converted,
    locale.overridden.length,
    locale.notConverted.length,
    locale.stale.length,
    locale.removed,
    locale.dead,
    locale.filled.length,
    locale.own,
    locale.missing.length,
  ];
  const widths = [8, 9, 12, 10, 6, 8, 5, 7, 4, 8];
  console.log(
    locale.tag.padEnd(8) +
      cells.map((cell, at) => String(cell).padStart(widths[at] ?? 8)).join(' '),
  );
  for (const { id, reason } of locale.notConverted) console.log(`         ${id}: ${reason}`);
}
