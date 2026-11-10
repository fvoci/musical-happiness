/**
 * Where the original lies, for the tools that read it.
 *
 * The web app was converted from two things of the original: the
 * translations of its web app (tools/i18n) and the archive of its built-in
 * templates (tools/templates). What the conversions wrote is committed
 * (`messages/`, `tools/templates/output/`), and the app is checked, tested
 * and built from that alone. Where the original is still there, the tools
 * also convert it again and hold the result against what is committed; where
 * it is not, they skip that and say so, with `skipNotice`.
 *
 * The places are the ones of the repository the port started in, unless the
 * environment names others:
 *
 * - `FVOCI_ORIGINAL_WEBAPP`: the folder of the original web app (it has
 *   `i18n/` and `src/`). Default: `webapp/` at the repository root.
 * - `FVOCI_TEMPLATE_ARCHIVE`: the archive of the built-in templates.
 *   Default: `server/assets/templates.boardarchive` at the repository root.
 *
 * The commands take the same two as arguments (`--original`, `--archive`).
 */
import { existsSync } from 'node:fs';
import path from 'node:path';

export const WEB_ROOT = path.resolve(import.meta.dirname, '..');
export const REPOSITORY_ROOT = path.resolve(WEB_ROOT, '../..');

export const ORIGINAL_WEBAPP = path.resolve(
  process.env.FVOCI_ORIGINAL_WEBAPP || path.join(REPOSITORY_ROOT, 'webapp'),
);
export const TEMPLATE_ARCHIVE = path.resolve(
  process.env.FVOCI_TEMPLATE_ARCHIVE ||
    path.join(REPOSITORY_ROOT, 'server/assets/templates.boardarchive'),
);

/** Whether a folder is the original web app, as far as the tools read it. */
export function hasOriginalWebapp(root: string = ORIGINAL_WEBAPP): boolean {
  return existsSync(path.join(root, 'i18n/en.json')) && existsSync(path.join(root, 'src/i18n.tsx'));
}

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
  if (!hasOriginalWebapp()) {
    notices.push(
      skipNotice(
        `the tests that convert the translations of the original again (${ORIGINAL_WEBAPP})`,
      ),
    );
  }
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
