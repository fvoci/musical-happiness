/**
 * The notices of the file viewer's HWP renderer, `@rhwp/core`.
 *
 * Its package names one licence, rhwp's own (MIT), and holds its licence
 * file. Its WebAssembly is rhwp built together with the Rust crates it
 * depends on, each under its own licence; their notices are in
 * `rhwp-wasm-crates.txt`, generated from rhwp's source at the installed
 * version with the repository's cargo-about configuration (the licences of
 * deny.toml: the generation fails on any other). Both are listed when the
 * module made from the WebAssembly is in the bundle (tools/build/rhwp-wasm.ts).
 *
 * After updating `@rhwp/core`, generate the file again from the new version
 * (the build fails until it names that version), from the repository root:
 *
 *   git clone --depth 1 --branch v<version> https://github.com/edwardkim/rhwp.git target/rhwp-src
 *   RUSTUP_TOOLCHAIN=stable cargo about generate -m target/rhwp-src/Cargo.toml -c about.toml \
 *     --target wasm32-unknown-unknown --locked --fail \
 *     apps/web/tools/licenses/rhwp-wasm.hbs -o apps/web/tools/licenses/rhwp-wasm-crates.txt
 *
 * (`RUSTUP_TOOLCHAIN` keeps rustup from installing the toolchain rhwp pins.)
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { packageDirectory, readPackageLicense, type PackageLicense } from './packages.ts';

const WASM_MODULE = 'virtual:rhwp-wasm';
const CRATES_FILE = path.join(import.meta.dirname, 'rhwp-wasm-crates.txt');

/** The licences of rhwp and of the crates in its WebAssembly, when that is in the bundle. */
export function rhwpWasmLicenses(bundledModules: readonly string[]): PackageLicense[] {
  if (!bundledModules.some((id) => id.includes(WASM_MODULE))) return [];
  const own = readPackageLicense('@rhwp/core');
  const { version } = JSON.parse(
    readFileSync(path.join(packageDirectory('@rhwp/core'), 'package.json'), 'utf8'),
  ) as { version: string };
  const text = readFileSync(CRATES_FILE, 'utf8').trim();
  if (!text.includes(`\n  rhwp ${version} `)) {
    throw new Error(
      `${CRATES_FILE} is not for @rhwp/core ${version}: generate it again (see rhwp-wasm.ts)`,
    );
  }
  const listed = /^Licenses: (.+)$/m.exec(text)?.[1]?.split(' ') ?? [];
  if (listed.length === 0) throw new Error(`${CRATES_FILE}: no "Licenses:" line`);
  return [
    own,
    {
      name: '@rhwp/core WebAssembly (Rust crates)',
      version,
      identifier: listed.join(' AND '),
      text,
    },
  ];
}
