/**
 * Third-party notices of a build.
 *
 * What is listed is what is in the output, not what is installed:
 *
 * - packages whose modules Vite bundled (its own licence report),
 * - packages whose style sheets are imported from src/styles,
 * - packages whose tools wrote source files into this project ("vendored"),
 * - the fonts the font tool produced.
 *
 * Every package has to carry a licence from the list in the repository's
 * `deny.toml`; there are no exceptions for anything that ships. The fonts are
 * under the SIL Open Font License, which that list does not have because no
 * Rust crate needs it; they are accepted by name here and nothing else is.
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import type { Plugin } from 'vite';

import exceptions from './exceptions.json' with { type: 'json' };
import { readPackageLicense, type PackageLicense } from './packages.ts';
import { isAllowed, readAllowedLicenses } from './policy.ts';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');
const LICENSE_REPORT = '.vite/license.json';
export const NOTICES_FILE = 'static/THIRD_PARTY_NOTICES.txt';

/** The only licence accepted for font files. */
const FONT_LICENSE = 'OFL-1.1';

export interface FontLicense {
  family: string;
  version: string;
  identifier: string;
  text: string;
}

/**
 * The licence of a bundled package whose `package.json` does not name one.
 * Nothing is guessed: the package has to be listed under `unlabelled` in
 * exceptions.json with the licence a person read off its licence file, and
 * that file still has to begin with the line recorded there.
 */
function labelled(item: PackageLicense, fail: (message: string) => never): PackageLicense {
  const entry = exceptions.unlabelled.find((candidate) => candidate.name === item.name);
  if (!entry) {
    return fail(
      `${item.name} ${item.version} is in the bundle and its package.json names no licence. Read its licence file and list it under "unlabelled" in tools/licenses/exceptions.json.`,
    );
  }
  const shipped = readPackageLicense(item.name);
  const firstLine = shipped.text.split('\n')[0]?.trim() ?? '';
  if (firstLine !== entry.firstLine) {
    return fail(
      `${item.name} ${item.version}: its licence file begins with "${firstLine}", not "${entry.firstLine}" as recorded in tools/licenses/exceptions.json. Read it again.`,
    );
  }
  return { ...item, identifier: entry.license, text: item.text || shipped.text };
}

/** Packages named by a bare `@import` in a style sheet of the app. */
export function styleSheetPackages(css: string): string[] {
  const names = new Set<string>();
  for (const match of css.matchAll(/@import\s+['"]([^'"./][^'"]*)['"]/g)) {
    const specifier = match[1] ?? '';
    const parts = specifier.split('/');
    names.add(specifier.startsWith('@') ? parts.slice(0, 2).join('/') : (parts[0] ?? ''));
  }
  return [...names].sort();
}

function section(title: string, identifier: string, text: string): string {
  const rule = '-'.repeat(72);
  const body = text === '' ? `(the package ships no licence file; licence: ${identifier})` : text;
  return `${rule}\n${title}\nLicense: ${identifier}\n${rule}\n\n${body}\n`;
}

export function renderNotices(packages: PackageLicense[], fonts: FontLicense[]): string {
  const parts = [
    'fvoci web app: third-party notices',
    '',
    'The web app contains code and fonts from the projects below.',
    '',
  ];
  const sorted = [...packages].sort((a, b) => a.name.localeCompare(b.name));
  for (const item of sorted) {
    parts.push(section(`${item.name} ${item.version}`, item.identifier, item.text));
  }
  for (const font of [...fonts].sort((a, b) => a.family.localeCompare(b.family))) {
    parts.push(section(`${font.family} ${font.version} (font)`, font.identifier, font.text));
  }
  return parts.join('\n');
}

/**
 * Checks the licences of everything in the bundle and writes the notices file.
 * Needs `build.license.fileName` to be `.vite/license.json`.
 */
export function thirdPartyNotices(): Plugin {
  return {
    name: 'fvoci:third-party-notices',
    apply: 'build',
    generateBundle: {
      order: 'post',
      handler(_options, bundle) {
        const report = bundle[LICENSE_REPORT];
        if (report?.type !== 'asset') {
          this.error(`${LICENSE_REPORT} is missing: set build.license.fileName to it`);
        }
        const bundled = (JSON.parse(String(report.source)) as PackageLicense[]).map((item) =>
          item.identifier ? item : labelled(item, (message) => this.error(message)),
        );
        // The raw report is an input of this step, not part of the product.
        Reflect.deleteProperty(bundle, LICENSE_REPORT);

        const byName = new Map(bundled.map((item) => [item.name, item]));
        const css = readFileSync(path.join(WEB_ROOT, 'src/styles/app.css'), 'utf8');
        const extra = [...styleSheetPackages(css), ...exceptions.vendored.map((item) => item.name)];
        for (const name of extra) {
          if (!byName.has(name)) byName.set(name, readPackageLicense(name));
        }

        const allowed = new Set(readAllowedLicenses());
        const refused = [...byName.values()].filter((item) => !isAllowed(item.identifier, allowed));
        if (refused.length > 0) {
          this.error(
            `packages in the bundle with a licence that deny.toml does not allow:\n  ${refused
              .map((item) => `${item.name} ${item.version}: ${item.identifier || '(none)'}`)
              .join('\n  ')}`,
          );
        }

        const fontsFile = path.join(WEB_ROOT, 'src/styles/fonts/licenses.json');
        const fonts = existsSync(fontsFile)
          ? (JSON.parse(readFileSync(fontsFile, 'utf8')) as FontLicense[])
          : [];
        const refusedFonts = fonts.filter((font) => font.identifier !== FONT_LICENSE);
        if (refusedFonts.length > 0) {
          this.error(
            `fonts with a licence other than ${FONT_LICENSE}: ${refusedFonts.map((font) => font.family).join(', ')}`,
          );
        }

        this.emitFile({
          type: 'asset',
          fileName: NOTICES_FILE,
          source: renderNotices([...byName.values()], fonts),
        });
        this.info(`notices: ${byName.size} packages, ${fonts.length} fonts`);
      },
    },
  };
}
