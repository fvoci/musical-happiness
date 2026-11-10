/**
 * The spikes: throw-away screens for trying out one hard part at a time.
 *
 *   pnpm run spikes         development server, pages under /spikes/<name>/
 *   pnpm run spikes:build   production build into dist-spikes/, for measuring size
 *
 * They share the app's styles, components and logic but are no part of the
 * product build: the product's vite.config.ts never sees these pages.
 */
import { readdirSync, existsSync } from 'node:fs';
import path from 'node:path';

import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

const root = import.meta.dirname;
const pages = Object.fromEntries(
  readdirSync(path.join(root, 'spikes'), { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isDirectory() && existsSync(path.join(root, 'spikes', entry.name, 'index.html')),
    )
    .map((entry) => [entry.name, path.join(root, 'spikes', entry.name, 'index.html')]),
);

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  resolve: {
    alias: { $lib: path.resolve(root, 'src/lib') },
  },
  server: { host: '127.0.0.1', strictPort: true },
  build: {
    outDir: 'dist-spikes',
    emptyOutDir: true,
    rolldownOptions: { input: pages },
  },
});
