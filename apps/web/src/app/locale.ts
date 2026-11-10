/**
 * The language the app is shown in.
 *
 * It is what the reader chose on this device, or else the first language of
 * the browser that the app has messages for, or else English. The choice is
 * remembered on the device (it is needed before any account is known, and
 * the server design keeps the language out of the account settings for now).
 * Changing it reloads the page: every text on screen is a message, and a
 * reload is the simple way to have all of them again.
 */
import { getLocale, locales, setLocale } from '$lib/i18n/paraglide/runtime.js';

export type Locale = (typeof locales)[number];

const STORAGE_KEY = 'fvoci:locale';

const isLocale = (tag: string | null | undefined): tag is Locale =>
  (locales as readonly string[]).includes(tag ?? '');

/** The message set closest to a language tag of the browser, if there is one. */
export function matchLocale(tag: string): Locale | undefined {
  if (isLocale(tag)) return tag;
  const lower = tag.toLowerCase();
  if (lower.startsWith('zh')) {
    return /-(hant|tw|hk|mo)\b/.test(lower) ? 'zh-Hant' : 'zh-Hans';
  }
  if (lower.startsWith('pt')) return 'pt-BR';
  const language = lower.split('-')[0];
  return isLocale(language) ? language : undefined;
}

function remembered(): Locale | undefined {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return isLocale(stored) ? stored : undefined;
  } catch {
    return undefined;
  }
}

/** Settles the language for this visit and tells the document. Call before anything is drawn. */
export function startLocale(): Locale {
  const fromBrowser = navigator.languages.map(matchLocale).find((tag) => tag !== undefined);
  const locale = remembered() ?? fromBrowser ?? 'en';
  void setLocale(locale, { reload: false });
  document.documentElement.lang = locale;
  return locale;
}

export function currentLocale(): Locale {
  return getLocale();
}

/** Changes the language. The page is loaded again in it. */
export function chooseLocale(locale: Locale): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    // Not remembered: the choice still holds for the page that is loaded next... if it could be told.
  }
  void setLocale(locale, { reload: true });
}

export { locales };
