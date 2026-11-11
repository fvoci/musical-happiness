/**
 * Where the original lies, for the tools that read it.
 *
 * The built-in templates were converted from the archive of the original
 * (tools/templates). What the conversion wrote is committed
 * (`tools/templates/output/`), and the app is checked, tested and built from
 * that alone. Where the archive is still there, the tools also convert it
 * again and hold the result against what is committed; where it is not, they
 * skip that and say so, with `skipNotice`.
 *
 * The archive is looked for at `server/assets/templates.boardarchive` of the
 * repository root, unless `FVOCI_TEMPLATE_ARCHIVE` or the argument
 * `--archive` names another place.
 */
import { existsSync } from 'node:fs';
import path from 'node:path';

export const WEB_ROOT = path.resolve(import.meta.dirname, '..');
export const REPOSITORY_ROOT = path.resolve(WEB_ROOT, '../..');

export const TEMPLATE_ARCHIVE = path.resolve(
  process.env.FVOCI_TEMPLATE_ARCHIVE ||
    path.join(REPOSITORY_ROOT, 'server/assets/templates.boardarchive'),
);

export function hasTemplateArchive(file: string = TEMPLATE_ARCHIVE): boolean {
  return existsSync(file);
}

/** How every line about a skipped step begins, so that a log can be searched for them. */
export const SKIP_PREFIX = 'skipped, the original is not here:';

/** The line a tool or a test prints for a step it skips for want of the original. */
export const skipNotice = (what: string): string => `${SKIP_PREFIX} ${what}`;

/**
 * The lines for the groups of tests that are skipped here for want of the
 * original; none where it is all there. The test files print the same lines,
 * but a reporter may swallow what a test prints, so the command that runs
 * the tests prints them too (tools/test-zones.ts).
 */
export function skippedTests(): string[] {
  const notices: string[] = [];
  if (!hasTemplateArchive()) {
    notices.push(
      skipNotice(`the tests that convert the template archive again (${TEMPLATE_ARCHIVE})`),
    );
  }
  return notices;
}

/** The value that follows a flag on the command line, if the flag is there. */
export function argumentAfter(
  flag: string,
  argv: readonly string[] = process.argv,
): string | undefined {
  const index = argv.indexOf(flag);
  return index === -1 ? undefined : argv[index + 1];
}
