/** The files that make the built Storybook a public site (tools/storybook/site.ts). */
import { describe, expect, it } from 'vitest';

import { scriptHash } from '../../tools/build/inline-scripts.ts';
import {
  REDIRECTS,
  ROBOTS,
  headersFile,
  pageScriptHashes,
  storybookPolicy,
  unhashedAssets,
} from '../../tools/storybook/site.ts';

/** The header lines of a `_headers` block: the indented lines after the line that is the path. */
function block(file: string, pattern: string): string[] {
  const lines = file.split('\n');
  const start = lines.indexOf(pattern);
  expect(start, `no block for ${pattern}`).toBeGreaterThanOrEqual(0);
  const end = lines.findIndex((line, index) => index > start && !line.startsWith('  '));
  return lines.slice(start + 1, end).map((line) => line.trim());
}

describe('the headers of the public Storybook', () => {
  const file = headersFile(["'sha256-one'", "'sha256-two'"]);

  it('keep every response out of search engines and send its type as it is', () => {
    expect(block(file, '/*')).toEqual(
      expect.arrayContaining([
        'X-Robots-Tag: noindex, nofollow',
        'X-Content-Type-Options: nosniff',
        'Referrer-Policy: no-referrer',
      ]),
    );
  });

  it('send the policy with the hashes of the inline scripts and nothing that runs any script', () => {
    const policy = block(file, '/*').find((line) => line.startsWith('Content-Security-Policy: '));
    expect(policy).toBe(
      `Content-Security-Policy: ${storybookPolicy(["'sha256-one'", "'sha256-two'"])}`,
    );
    const directives = new Map(
      storybookPolicy(["'sha256-one'"])
        .split('; ')
        .map((directive) => [directive.split(' ')[0], directive.split(' ').slice(1)] as const),
    );
    expect(directives.get('script-src')).toEqual(["'self'", "'sha256-one'"]);
    expect(directives.get('frame-ancestors')).toEqual(["'self'"]);
    expect(directives.get('object-src')).toEqual(["'none'"]);
    expect([...directives.values()].flat()).not.toContain("'unsafe-eval'");
    expect(directives.get('script-src')).not.toContain("'unsafe-inline'");
  });

  it('keep the hashed files under assets/ for a year, and only those', () => {
    expect(block(file, '/assets/*')).toEqual([
      'Cache-Control: public, max-age=31536000, immutable',
    ]);
    expect(file.match(/Cache-Control/g)).toHaveLength(1);
  });
});

describe('the other files of the site', () => {
  it('answer `/` with the manager without a redirect', () => {
    expect(REDIRECTS).toBe('/ /index.html 200\n');
  });

  it('ask every crawler to stay away', () => {
    expect(ROBOTS).toBe('User-agent: *\nDisallow: /\n');
  });
});

describe('the build the site is made from', () => {
  it('names the files under assets/ that lack the hash a year in the cache needs', () => {
    expect(
      unhashedAssets([
        'iframe-DKX11vrD.js',
        'Account.stories-V7fputDo.js',
        'noto-sans-kr-1-Bx_9-k2Q.woff2',
        'logo.svg',
      ]),
    ).toEqual(['logo.svg']);
  });

  it('hashes the inline scripts of both pages, each once', () => {
    const manager = '<script>window.A = 1;</script><script type="module" src="./x.js"></script>';
    const frame = '<script>window.A = 1;</script><script type="module">import "./y.js";</script>';
    expect(pageScriptHashes([manager, frame])).toEqual([
      scriptHash('window.A = 1;'),
      scriptHash('import "./y.js";'),
    ]);
  });
});
