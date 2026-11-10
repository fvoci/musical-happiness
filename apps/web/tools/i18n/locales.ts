/**
 * The locales of the web app and where each comes from in the original.
 *
 * The original has 38 translation files but offers 17 locales: the ones its
 * `webapp/src/i18n.tsx` imports (the same 17 are listed for the language menu
 * in `webapp/src/constants.ts`). Those are converted. Korean is not among
 * them; it is converted as well because it is a locale of this app.
 * `bundledFiles` reads the list from the original, and the conversion fails
 * when it is not the one written here.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

export const WEB_ROOT = path.resolve(import.meta.dirname, '../..');
export const REPOSITORY_ROOT = path.resolve(WEB_ROOT, '../..');
export const ORIGINAL_ROOT = path.join(REPOSITORY_ROOT, 'webapp');

export const BASE_LOCALE = 'en';
/** Has to have every key the base locale has. */
export const COMPLETE_LOCALES = ['ko'];

export interface Locale {
  /** BCP 47, the name of the locale in this app. */
  tag: string;
  /** `webapp/i18n/<file>.json`. */
  file: string;
  /** The code the original stored as the user's language; undefined when it did not offer the locale. */
  originalCode: string | undefined;
}

export const LOCALES: Locale[] = [
  { tag: 'en', file: 'en', originalCode: 'en' },
  { tag: 'ko', file: 'ko', originalCode: undefined },
  { tag: 'ca', file: 'ca', originalCode: 'ca' },
  { tag: 'de', file: 'de', originalCode: 'de' },
  { tag: 'el', file: 'el', originalCode: 'el' },
  { tag: 'es', file: 'es', originalCode: 'es' },
  { tag: 'fr', file: 'fr', originalCode: 'fr' },
  { tag: 'id', file: 'id', originalCode: 'id' },
  { tag: 'it', file: 'it', originalCode: 'it' },
  { tag: 'ja', file: 'ja', originalCode: 'ja' },
  { tag: 'nl', file: 'nl', originalCode: 'nl' },
  { tag: 'oc', file: 'oc', originalCode: 'oc' },
  { tag: 'pt-BR', file: 'pt_BR', originalCode: 'pt-br' },
  { tag: 'ru', file: 'ru', originalCode: 'ru' },
  { tag: 'sv', file: 'sv', originalCode: 'sv' },
  { tag: 'tr', file: 'tr', originalCode: 'tr' },
  { tag: 'zh-Hans', file: 'zh_Hans', originalCode: 'zh-cn' },
  { tag: 'zh-Hant', file: 'zh_Hant', originalCode: 'zh-tw' },
];

/** The translation files the original imports, by the names in `webapp/i18n`. */
export function bundledFiles(originalRoot: string = ORIGINAL_ROOT): string[] {
  const source = readFileSync(path.join(originalRoot, 'src/i18n.tsx'), 'utf8');
  return [...source.matchAll(/from '\.\.\/i18n\/([^']+)\.json'/g)]
    .map((match) => match[1] ?? '')
    .sort();
}
