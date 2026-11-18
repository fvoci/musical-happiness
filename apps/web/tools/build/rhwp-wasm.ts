/**
 * The HWP renderer's WebAssembly for the file viewer's frame, which cannot
 * fetch it (src/viewer-frame/hwp.ts).
 *
 * The module `virtual:rhwp-wasm` holds the bytes of `@rhwp/core`'s
 * `rhwp_bg.wasm` as hexadecimal text, its default export:
 *
 *   export default '0061736d…';
 *
 * Hexadecimal rather than Base64: it is twice the size before compression
 * and smaller after it, as each byte stays a pair of characters the
 * compressors see repeat (Brotli: 3.3 MB against 4.2 MB for Base64 and
 * 3.1 MB for the WebAssembly itself, rhwp 0.8.7).
 *
 * Its memory is given a maximum (`RHWP_MEMORY_LIMIT_BYTES`, wasm-memory.ts):
 * past it rhwp traps, and the worker it runs in is stopped, rather than a
 * file growing it until the tab's process runs out. A page with the largest
 * picture a file the viewer takes can hold needs about 650 MB (rhwp 0.8.7).
 *
 * The package's own loader would fetch the file from beside itself when it is
 * given nothing to load (`new URL('rhwp_bg.wasm', import.meta.url)`), and
 * that address alone makes the build copy the 12 MB file into the output. The
 * worker always gives it the bytes, so that branch is cut out of the loader;
 * the build fails if the loader no longer reads as expected.
 */
import { readFileSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

import type { Plugin } from 'vite';

import { withMemoryLimit } from './wasm-memory.ts';

const ID = 'virtual:rhwp-wasm';
const RESOLVED = `\0${ID}`;

/** The loader's fallback to an address beside it, in `rhwp.js`. */
const DEFAULT_ADDRESS = "module_or_path = new URL('rhwp_bg.wasm', import.meta.url);";

/** The most memory rhwp's WebAssembly may take, in bytes. */
export const RHWP_MEMORY_LIMIT_BYTES = 1024 * 1024 * 1024;

/** The folder of the installed `@rhwp/core`. */
export function rhwpDirectory(): string {
  const require = createRequire(import.meta.url);
  return realpathSync(path.dirname(require.resolve('@rhwp/core')));
}

/** The file of rhwp's WebAssembly, as the package has it. */
export const rhwpWasmFile = (): string => path.join(rhwpDirectory(), 'rhwp_bg.wasm');

/** rhwp's WebAssembly as the frame gets it: its memory held to the limit, as hexadecimal text. */
export function rhwpWasmHex(limit = RHWP_MEMORY_LIMIT_BYTES): string {
  return Buffer.from(withMemoryLimit(readFileSync(rhwpWasmFile()), limit)).toString('hex');
}

export function rhwpWasm(): Plugin {
  return {
    name: 'fvoci:rhwp-wasm',
    resolveId(id) {
      return id === ID ? RESOLVED : undefined;
    },
    load(id) {
      if (id !== RESOLVED) return undefined;
      this.addWatchFile(rhwpWasmFile());
      return {
        code: `export default ${JSON.stringify(rhwpWasmHex())};`,
        moduleSideEffects: false,
      };
    },
    transform(code, id) {
      if (path.basename(id.split('?')[0] ?? '') !== 'rhwp.js' || !id.includes('@rhwp')) {
        return undefined;
      }
      if (!code.includes(DEFAULT_ADDRESS)) {
        this.error(
          `@rhwp/core: rhwp.js no longer reads "${DEFAULT_ADDRESS}"; see ${import.meta.filename}`,
        );
      }
      const replaced = code.replace(
        DEFAULT_ADDRESS,
        "throw new Error('rhwp: the WebAssembly is given to init, never fetched');",
      );
      return { code: replaced, map: null };
    },
  };
}
