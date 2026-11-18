import { describe, expect, it } from 'vitest';

import { hasContentHash } from '../../tools/build/output.ts';
import { styleSheetPackages } from '../../tools/licenses/notices.ts';
import { isAllowed, readAllowedLicenses } from '../../tools/licenses/policy.ts';
import { packageDirectoryOf } from '../../tools/licenses/worker-packages.ts';

describe('licence policy', () => {
  const allowed = new Set(readAllowedLicenses());

  it('reads the list from deny.toml', () => {
    expect(allowed.has('MIT')).toBe(true);
    expect(allowed.has('Apache-2.0 WITH LLVM-exception')).toBe(true);
    expect(allowed.size).toBeGreaterThanOrEqual(10);
  });

  it('has no copyleft licence on the list', () => {
    for (const name of allowed) expect(name).not.toMatch(/GPL|MPL|EPL|CDDL|EUPL/);
  });

  it.each([
    ['MIT', true],
    ['(MIT OR CC0-1.0)', true],
    ['MIT OR GPL-3.0-only', true],
    ['MIT AND GPL-3.0-only', false],
    ['(Apache-2.0 WITH LLVM-exception)', true],
    ['Apache-2.0 WITH Classpath-exception-2.0', false],
    ['(MIT AND Zlib) OR GPL-2.0-only', true],
    ['MPL-2.0', false],
    ['GPL-3.0-or-later', false],
    ['SEE LICENSE IN LICENSE.md', false],
    ['', false],
    ['MIT OR', false],
    ['(MIT', false],
  ])('%j is allowed: %s', (expression, expected) => {
    expect(isAllowed(expression, allowed)).toBe(expected);
  });
});

describe('packages named by style sheet imports', () => {
  it('finds bare specifiers and ignores relative ones', () => {
    const css = `@import 'tailwindcss';\n@import "tw-animate-css";\n@import './fonts.css';\n@import '@scope/pkg/file.css';\n@import 'shadcn-svelte/tailwind.css';`;
    expect(styleSheetPackages(css)).toEqual([
      '@scope/pkg',
      'shadcn-svelte',
      'tailwindcss',
      'tw-animate-css',
    ]);
  });
});

describe('content hash in a file name, as the server sees it', () => {
  it.each([
    ['index-5f6bf5c286feeaf1.js', true],
    ['noto-sans-kr-common-0a1b2c3d4e5f6071.woff2', true],
    ['index-DP28MaF1.js', false],
    ['index-abcdefab.js', false],
    ['index-1234567.js', false],
    ['THIRD_PARTY_NOTICES.txt', false],
  ])('%s: %s', (name, expected) => {
    expect(hasContentHash(name)).toBe(expected);
  });
});

describe("the packages of the app's workers", () => {
  it('are found by the path of each module', () => {
    const store = '/w/node_modules/.pnpm/saxes@6.0.0/node_modules';
    expect(packageDirectoryOf(`${store}/saxes/lib/saxes.js`)).toBe(`${store}/saxes`);
    expect(packageDirectoryOf(`${store}/@scope/name/dist/a.js?inline`)).toBe(
      `${store}/@scope/name`,
    );
    expect(packageDirectoryOf('/w/apps/web/src/viewer-frame/pptx-worker.ts')).toBeUndefined();
    expect(packageDirectoryOf('\0vite/preload-helper.js')).toBeUndefined();
  });
});
