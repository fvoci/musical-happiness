/**
 * Deploys the public Storybook, or makes a Preview of it, with the pinned
 * Wrangler of tools/storybook/wrangler/.
 *
 *   node tools/storybook/wrangler.ts deploy|preview [<argument> ...]
 *
 * Wrangler is not a dependency of the web app: it brings workerd and sharp,
 * some 200 MB that no other install needs. It has a workspace of its own
 * there, installed here from its lockfile, with no install scripts.
 *
 * Nothing goes out unless the folder is the site `storybook:site` makes: a
 * plain `build-storybook` has no `_headers`, and would be served without
 * `noindex` and without its policy.
 */
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');
const WRANGLER = path.join(import.meta.dirname, 'wrangler');
const SITE_FILES = ['_headers', '_redirects', 'robots.txt'].map((file) =>
  path.join(WEB_ROOT, 'storybook-static', file),
);

const [command, ...rest] = process.argv.slice(2);
if (command !== 'deploy' && command !== 'preview') {
  console.error('usage: node tools/storybook/wrangler.ts deploy|preview [<argument> ...]');
  process.exit(2);
}
const missing = SITE_FILES.filter((file) => !existsSync(file));
if (missing.length > 0) {
  console.error(
    `storybook-static is not the site: no ${missing.map((file) => path.basename(file)).join(', ')} (run storybook:site)`,
  );
  process.exit(1);
}
// Under `pnpm run`, the pnpm that runs the script; it may be a script for Node.
const pnpm = process.env.npm_execpath;
const [program, ...first] =
  pnpm === undefined ? ['pnpm'] : /\.[cm]?js$/.test(pnpm) ? [process.execPath, pnpm] : [pnpm];
execFileSync(program, [...first, 'install', '--frozen-lockfile'], {
  cwd: WRANGLER,
  stdio: 'inherit',
});
execFileSync(path.join(WRANGLER, 'node_modules/.bin/wrangler'), [command, ...rest], {
  cwd: WEB_ROOT,
  stdio: 'inherit',
});
