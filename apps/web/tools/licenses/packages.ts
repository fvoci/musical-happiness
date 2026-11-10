/** Reading the licence of an installed package from `node_modules`. */
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

export interface PackageLicense {
  name: string;
  version: string;
  /** SPDX expression from `package.json`. */
  identifier: string;
  /** Text of the licence file shipped in the package, or empty. */
  text: string;
}

const require = createRequire(path.resolve(import.meta.dirname, '../../package.json'));

/** Directory of a package that the web app depends on directly. */
export function packageDirectory(name: string): string {
  // `exports` of some packages hide package.json, so walk up from any entry.
  const searchPaths = require.resolve.paths(name) ?? [];
  for (const base of searchPaths) {
    const candidate = path.join(base, name);
    if (existsSync(path.join(candidate, 'package.json'))) return candidate;
  }
  throw new Error(`package ${name} is not installed`);
}

export function readPackageLicense(name: string): PackageLicense {
  const directory = packageDirectory(name);
  const manifest = JSON.parse(readFileSync(path.join(directory, 'package.json'), 'utf8')) as {
    name: string;
    version: string;
    license?: string;
  };
  const licenseFile = readdirSync(directory).find((file) =>
    /^(licen[sc]e|copying)(\.|$)/i.test(file),
  );
  return {
    name: manifest.name,
    version: manifest.version,
    identifier: manifest.license ?? '',
    text: licenseFile ? readFileSync(path.join(directory, licenseFile), 'utf8').trim() : '',
  };
}
