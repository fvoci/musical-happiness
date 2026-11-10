import path from 'node:path';

import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

import { checkHashedNames, precompress } from './tools/build/output.ts';
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

export default defineConfig(({ command }) => ({
  plugins: [
    tailwindcss(),
    svelte(),
    svelteTesting(),
    thirdPartyNotices(),
    checkHashedNames(),
    precompress(),
  ],
  resolve: {
    alias: { $lib: path.resolve(import.meta.dirname, 'src/lib') },
  },
  experimental:
    command === 'build'
      ? {
          renderBuiltUrl(fileName, { hostType }) {
            if (hostType === 'html') return `${BASE_URL_PLACEHOLDER}/${fileName}`;
            return { relative: true };
          },
        }
      : {},
  build: {
    outDir: 'dist',
    assetsDir: 'static',
    // Raw data for tools/licenses: which packages ended up in the bundle.
    license: { fileName: '.vite/license.json' },
    rolldownOptions: {
      output: {
        // Hexadecimal, because that is what the server recognises as a
        // content hash. Sixteen digits: a hash of letters a-f only, which the
        // server would not recognise, has a chance of 1 in 6.6 million.
        hashCharacters: 'hex',
        entryFileNames: 'static/[name]-[hash:16].js',
        chunkFileNames: 'static/[name]-[hash:16].js',
        assetFileNames: 'static/[name]-[hash:16][extname]',
      },
    },
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
