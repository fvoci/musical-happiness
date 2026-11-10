/**
 * Build steps that shape the output directory for the server.
 *
 * The server sends `Cache-Control: immutable` for a file under `static/` whose
 * name carries a content hash, and it sends `<file>.br` or `<file>.gz` instead
 * of `<file>` when one lies next to it and the client accepts the encoding.
 * `checkHashedNames` fails the build when a file would miss the first rule,
 * `precompress` writes the files for the second.
 */
import { readdir, readFile, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { brotliCompress, constants, gzip } from 'node:zlib';

import type { Plugin } from 'vite';

const brotli = promisify(brotliCompress);
const gzipAsync = promisify(gzip);

/**
 * The server's test for "this name contains a content hash": some part
 * between `-`, `.` and `_` is at least eight hexadecimal digits long and has
 * a decimal digit in it.
 */
export function hasContentHash(fileName: string): boolean {
  return fileName
    .split(/[-._]/)
    .some((part) => part.length >= 8 && /^[0-9a-fA-F]+$/.test(part) && /[0-9]/.test(part));
}

/** Files that are meant to be revalidated on every request, by name. */
const UNHASHED_ON_PURPOSE = new Set(['THIRD_PARTY_NOTICES.txt']);

/** Already compressed formats; compressing them again gains nothing. */
const COMPRESSIBLE = new Set([
  '.css',
  '.html',
  '.js',
  '.json',
  '.map',
  '.mjs',
  '.svg',
  '.txt',
  '.webmanifest',
  '.xml',
]);

async function walk(directory: string): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(full)));
    else if (entry.isFile()) files.push(full);
  }
  return files.sort();
}

function outputDirectory(plugin: {
  environment: { config: { root: string; build: { outDir: string } } };
}): string {
  const { root, build } = plugin.environment.config;
  return path.resolve(root, build.outDir);
}

/** Fails the build when a file under `static/` has no content hash in its name. */
export function checkHashedNames(): Plugin {
  return {
    name: 'fvoci:check-hashed-names',
    apply: 'build',
    async closeBundle() {
      const staticDirectory = path.join(outputDirectory(this), 'static');
      const unhashed = (await walk(staticDirectory))
        .map((file) => path.relative(staticDirectory, file))
        .filter((file) => !/\.(br|gz)$/.test(file))
        .filter((file) => !UNHASHED_ON_PURPOSE.has(file) && !hasContentHash(path.basename(file)));
      if (unhashed.length > 0) {
        this.error(
          `files under static/ without a content hash in the name (the server would not cache them):\n  ${unhashed.join('\n  ')}`,
        );
      }
    },
  };
}

/**
 * Writes `<file>.br` and `<file>.gz` next to every compressible file under
 * `static/`, unless the compressed form is not smaller.
 *
 * `index.html` is left alone: the server fills the base URL into it on every
 * request and never sends a file from disk for it.
 */
export function precompress(): Plugin {
  return {
    name: 'fvoci:precompress',
    apply: 'build',
    async closeBundle() {
      const staticDirectory = path.join(outputDirectory(this), 'static');
      let written = 0;
      for (const file of await walk(staticDirectory)) {
        if (!COMPRESSIBLE.has(path.extname(file))) continue;
        const source = await readFile(file);
        const [br, gz] = await Promise.all([
          brotli(source, {
            params: {
              [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY,
              [constants.BROTLI_PARAM_SIZE_HINT]: source.length,
            },
          }),
          gzipAsync(source, { level: constants.Z_BEST_COMPRESSION }),
        ]);
        if (br.length < source.length) {
          await writeFile(`${file}.br`, br);
          written += 1;
        }
        if (gz.length < source.length) {
          await writeFile(`${file}.gz`, gz);
          written += 1;
        }
      }
      this.info(`precompress: wrote ${written} .br/.gz files`);
    },
  };
}

/** Sizes of the files of a build, for the report: `[relative path, bytes]`. */
export async function outputSizes(directory: string): Promise<[string, number][]> {
  const sizes: [string, number][] = [];
  for (const file of await walk(directory)) {
    sizes.push([path.relative(directory, file), (await stat(file)).size]);
  }
  return sizes;
}
