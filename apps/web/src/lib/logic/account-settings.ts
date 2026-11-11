/** Every account setting at its default, as a new account has them. */
import { DEFAULT_DISPLAY_SETTINGS, type DisplaySettings } from './display.ts';
import { DEFAULT_LANGUAGE_SETTINGS, type LanguageSettings } from './language.ts';
import { DEFAULT_REGIONAL_SETTINGS, type RegionalSettings } from './regional.ts';

export const DEFAULT_ACCOUNT_SETTINGS: RegionalSettings & DisplaySettings & LanguageSettings = {
  ...DEFAULT_REGIONAL_SETTINGS,
  ...DEFAULT_DISPLAY_SETTINGS,
  ...DEFAULT_LANGUAGE_SETTINGS,
};
