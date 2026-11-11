/**
 * The language of the interface as an account setting (`language`): `auto`
 * follows the browser, anything else is a language tag in the shape the
 * browsers' `Intl` reads (Unicode BCP 47). The server checks the shape only
 * and answers a tag as it was given; which messages a tag is shown with is
 * the app's to decide (`src/app/locale.ts`).
 */

/** The setting that follows the browser. */
export const AUTO_LANGUAGE = 'auto';

/** The longest tag the server takes. */
export const LANGUAGE_MAX_LENGTH = 64;

export interface LanguageSettings {
  /** `auto`, or a language tag such as `ko` or `pt-BR`. */
  language: string;
}

export const DEFAULT_LANGUAGE_SETTINGS: LanguageSettings = { language: AUTO_LANGUAGE };

/**
 * Whether the setting is one the server keeps: `auto`, or a tag `Intl` reads
 * of at most 64 characters. "Auto" or "en-US-US" is not.
 */
export function isLanguageSetting(value: string): boolean {
  if (value === AUTO_LANGUAGE) return true;
  if (value === '' || value.length > LANGUAGE_MAX_LENGTH) return false;
  try {
    return Intl.getCanonicalLocales(value).length === 1;
  } catch {
    return false;
  }
}

/** The setting out of what the server sent: one that is missing or has no shape reads as `auto`. */
export function readLanguageSettings(stored: unknown): LanguageSettings {
  const { language } = (typeof stored === 'object' && stored !== null ? stored : {}) as Record<
    string,
    unknown
  >;
  return {
    language:
      typeof language === 'string' && isLanguageSetting(language) ? language : AUTO_LANGUAGE,
  };
}
