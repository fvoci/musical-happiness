/**
 * Where the original uses each message id.
 *
 * An id counts as used by a file when the file has it as a whole string
 * literal. That is how the original names a message everywhere
 * (`id: '…'`, `id='…'`); the one id it builds at run time,
 * `Sidebar.${theme.id}`, names messages that are in no translation file.
 * Tests and snapshots are not looked at.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true })
    .sort((a, b) => (a.name < b.name ? -1 : 1))
    .flatMap((entry) => {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) return entry.name === '__snapshots__' ? [] : sourceFiles(file);
      return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [file] : [];
    });
}

/** For each id, the files under `sourceRoot` that use it, as paths relative to `relativeTo`. */
export function scanUsage(
  ids: readonly string[],
  sourceRoot: string,
  relativeTo: string,
): Map<string, string[]> {
  const files = sourceFiles(sourceRoot).map((file) => ({
    name: path.relative(relativeTo, file).split(path.sep).join('/'),
    text: readFileSync(file, 'utf8'),
  }));
  return new Map(
    ids.map((id) => [
      id,
      files
        .filter(({ text }) => ["'", '"', '`'].some((quote) => text.includes(quote + id + quote)))
        .map(({ name }) => name),
    ]),
  );
}
