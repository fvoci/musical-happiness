/**
 * Writes `messages/<locale>.json` from their sources.
 *
 *   node tools/i18n/convert.ts            write the message files
 *   node tools/i18n/convert.ts --check    fail when they are not what would be written
 *   node tools/i18n/convert.ts --report   print everything the conversion found, as JSON
 *
 * `--original <folder>` names the original web app. Without it the folder is
 * looked for where tools/original.ts says.
 *
 * The message files are generated. What is edited by hand is the sources:
 * `messages-own/` (messages only this app has), `messages-fill/`
 * (translations the original lacks), `messages-override/` (texts that replace
 * one of the original) and `tools/i18n/removed.json`. See conversion.ts, and
 * docs/i18n-conversion.md for what was found.
 *
 * Where the original is there, its translations are converted again
 * (conversion.ts). Where it is not, the message files themselves stand in
 * for it (standalone.ts): the texts they have for the ids of the original
 * are kept, and the hand-edited sources are applied to them as before. A
 * line says which of the two a run did. `--check` with the original also
 * makes sure that the second way gives the same files, so that the original
 * can be taken away at any time.
 *
 * `project.inlang/settings.json` is not written, but its locales have to be
 * the ones of locales.ts.
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { argumentAfter, hasOriginalWebapp, skipNotice } from '../original.ts';

import { convertAll } from './conversion.ts';
import { LOCALES, ORIGINAL_ROOT, REPOSITORY_ROOT, WEB_ROOT } from './locales.ts';
import { convertStandalone } from './standalone.ts';

const MODES = ['--check', '--report'];
const original = path.resolve(argumentAfter('--original') ?? ORIGINAL_ROOT);
const flags = process.argv.slice(2).filter((argument, at, all) => all[at - 1] !== '--original');
const unknown = flags.filter((flag) => flag !== '--original' && !MODES.includes(flag));
if (unknown.length > 0 || flags.filter((flag) => MODES.includes(flag)).length > 1) {
  console.error(`unknown arguments: ${(unknown.length > 0 ? unknown : flags).join(' ')}`);
  process.exit(2);
}
const mode = flags.find((flag) => MODES.includes(flag));

const withOriginal = hasOriginalWebapp(original);
const { files, report } = withOriginal
  ? convertAll(original, WEB_ROOT, REPOSITORY_ROOT)
  : convertStandalone();

if (mode === '--report') {
  console.log(JSON.stringify(report, null, 2));
  process.exit(0);
}

if (!withOriginal) {
  console.log(
    skipNotice(
      `converting the translations of the original again (${original}); the message files stand in for it`,
    ),
  );
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
  if (withOriginal) {
    // The files are current. Read as a source, they have to give themselves.
    const alone = convertStandalone().files;
    const differs = [...files].filter(([name, content]) => alone.get(name) !== content);
    if (differs.length > 0) {
      console.error(
        `the message files could not stand in for the original; without it these would come out differently:\n  ${differs.map(([name]) => name).join('\n  ')}`,
      );
      process.exit(1);
    }
  }
  console.log(
    `message files: up to date (${files.size} locales, ${
      withOriginal
        ? 'converted from the original, and the same without it'
        : 'held against the message files and the hand-edited sources'
    })`,
  );
  process.exit(0);
}

for (const [name, content] of files) writeFileSync(path.join(WEB_ROOT, name), content);

if (!withOriginal) {
  console.log(`English: ${report.kept.length} ids of the original kept\n`);
  console.log('locale   messages  overridden  filled  own  missing');
  for (const locale of report.locales) {
    const cells = [
      locale.converted + locale.filled.length + locale.own,
      locale.overridden.length,
      locale.filled.length,
      locale.own,
      locale.missing.length,
    ];
    const widths = [8, 11, 7, 4, 8];
    console.log(
      locale.tag.padEnd(8) +
        cells.map((cell, at) => String(cell).padStart(widths[at] ?? 8)).join(' '),
    );
  }
  process.exit(0);
}

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
