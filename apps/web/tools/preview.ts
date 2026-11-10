/**
 * Serves `dist/` the way the server does, for looking at a build without it.
 *
 *   node tools/preview.ts [port] [base path]
 *
 * Every path that is not under `static/` answers with `index.html`, with the
 * base path filled into the placeholder. Files under `static/` are sent with
 * the cache rule the server uses, and a `.br`, `.zst` or `.gz` file next to
 * one is sent in its place when the client accepts it (build/encodings.ts
 * has the order). It is a development aid: the end-to-end tests of the
 * product run against the real server.
 */
import { existsSync, readFileSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import path from 'node:path';

import { chooseEncoding } from './build/encodings.ts';
import { hasContentHash } from './build/output.ts';

const DIST = path.resolve(import.meta.dirname, '../dist');
const port = Number(process.argv[2] ?? 4173);
const basePath = (process.argv[3] ?? '').replace(/\/$/, '');

const TYPES: Record<string, string> = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
  '.woff2': 'font/woff2',
};

createServer((request, response) => {
  const url = new URL(request.url ?? '/', 'http://localhost');
  const pathname = decodeURIComponent(url.pathname);
  const inApp = basePath === '' || pathname === basePath || pathname.startsWith(`${basePath}/`);
  const relative = inApp ? pathname.slice(basePath.length) : pathname;

  if (inApp && relative.startsWith('/static/')) {
    const file = path.join(DIST, relative);
    if (
      !file.startsWith(path.join(DIST, 'static')) ||
      !existsSync(file) ||
      !statSync(file).isFile()
    ) {
      response.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' }).end('not found');
      return;
    }
    const encoding = chooseEncoding(request.headers['accept-encoding'], (extension) =>
      existsSync(file + extension),
    );
    const headers: Record<string, string> = {
      'Content-Type': TYPES[path.extname(file)] ?? 'application/octet-stream',
      'Cache-Control': hasContentHash(path.basename(file))
        ? 'public, max-age=31536000, immutable'
        : 'no-cache',
      Vary: 'Accept-Encoding',
    };
    if (encoding) {
      headers['Content-Encoding'] = encoding.name;
      response.writeHead(200, headers).end(readFileSync(file + encoding.extension));
    } else {
      response.writeHead(200, headers).end(readFileSync(file));
    }
    return;
  }

  const index = readFileSync(path.join(DIST, 'index.html'), 'utf8');
  response
    .writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-cache' })
    .end(index.replaceAll('{{.BaseURL}}', basePath));
}).listen(port, '127.0.0.1', () => {
  console.log(`preview: http://127.0.0.1:${port}${basePath}/`);
});
