import path from 'node:path';

import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

import { cspHashes } from './tools/build/inline-scripts.ts';
import { checkHashedNames, precompress } from './tools/build/output.ts';
import { pdfjsData } from './tools/build/pdfjs-data.ts';
import { thirdPartyNotices } from './tools/licenses/notices.ts';

/**
 * The server serves `index.html` for every page of the app and everything
 * else from `static/`. It also decides the path the app lives under, at run
 * time: it replaces this placeholder in `index.html` with that path (empty for
 * the root). So the build must not fix a base path: `index.html` refers to its
 * files through the placeholder, and scripts and style sheets refer to each
 * other relatively.
 */
const BASE_URL_PLACEHOLDER = '{{.BaseURL}}';

/**
 * The document the file viewer draws in (src/viewer-frame/), a page of its
 * own. It is built under `static/`, the one folder the server serves as
 * files, and is served as it is: it refers to its files relatively, as it
 * lies beside them.
 */
const VIEWER_FRAME = 'static/viewer-frame.html';

/** Names of the output files: hexadecimal hashes, which the server recognises (see `build`). */
const OUTPUT_NAMES = {
  hashCharacters: 'hex',
  entryFileNames: 'static/[name]-[hash:16].js',
  chunkFileNames: 'static/[name]-[hash:16].js',
  assetFileNames: 'static/[name]-[hash:16][extname]',
} as const;

export default defineConfig(({ command }) => ({
  plugins: [
    tailwindcss(),
    svelte(),
    svelteTesting(),
    pdfjsData(),
    thirdPartyNotices(),
    checkHashedNames(),
    precompress(),
    cspHashes(),
  ],
  resolve: {
    alias: [
      { find: '$lib', replacement: path.resolve(import.meta.dirname, 'src/lib') },
      // The diagram library's ELK layout engine is under a licence the project
      // does not allow, so it is left out of the build: this stub takes its
      // place (see the file). The notices step fails the build if the real
      // package is ever bundled.
      {
        find: /^elkjs(\/.*)?$/,
        replacement: path.resolve(import.meta.dirname, 'src/lib/editor/no-elk.ts'),
      },
    ],
  },
  experimental:
    command === 'build'
      ? {
          renderBuiltUrl(fileName, { hostId, hostType }) {
            if (hostType !== 'html') return { relative: true };
            if (hostId === VIEWER_FRAME) {
              return `./${path.posix.relative(path.posix.dirname(VIEWER_FRAME), fileName)}`;
            }
            return `${BASE_URL_PLACEHOLDER}/${fileName}`;
          },
        }
      : {},
  build: {
    outDir: 'dist',
    assetsDir: 'static',
    // Every asset is a file of its own. Vite would otherwise write a small
    // one (the smallest formula font) into the style sheet as a `data:`
    // address, which the page's content security policy does not allow.
    assetsInlineLimit: 0,
    // Raw data for tools/licenses: which packages ended up in the bundle.
    license: { fileName: '.vite/license.json' },
    rolldownOptions: {
      input: {
        index: path.resolve(import.meta.dirname, 'index.html'),
        'viewer-frame': path.resolve(import.meta.dirname, VIEWER_FRAME),
      },
      // Hexadecimal, because that is what the server recognises as a
      // content hash. Sixteen digits: a hash of letters a-f only, which the
      // server would not recognise, has a chance of 1 in 6.6 million.
      output: OUTPUT_NAMES,
    },
  },
  // pdf.js's worker, which the viewer's frame starts from its own text
  // (src/viewer-frame/pdf-worker.ts): a classic script, as a `blob:` worker
  // of an opaque origin cannot load modules.
  worker: {
    format: 'iife',
    rolldownOptions: { output: OUTPUT_NAMES },
  },
  test: {
    projects: [
      {
        extends: true,
        test: {
          name: 'logic',
          environment: 'node',
          include: [
            'tests/logic/**/*.test.ts',
            'tests/editor/**/*.test.ts',
            'tests/tokens/**/*.test.ts',
            'tests/tools/**/*.test.ts',
          ],
        },
      },
      {
        extends: true,
        test: {
          name: 'ui',
          environment: 'jsdom',
          include: ['tests/ui/**/*.test.ts', 'tests/state/**/*.test.ts'],
          setupFiles: ['tests/ui/setup.ts'],
        },
      },
    ],
  },
}));
