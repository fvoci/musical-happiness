import type { Preview } from '@storybook/svelte';

import { loadFontFaces } from '$lib/fonts.js';
import { setLocale } from '$lib/i18n/paraglide/runtime.js';
import { applyTheme } from '$lib/theme.svelte.js';

import '../src/styles/app.css';

const preview: Preview = {
  globalTypes: {
    locale: {
      description: 'Language',
      toolbar: {
        icon: 'globe',
        items: [
          { value: 'en', title: 'English' },
          { value: 'ko', title: '한국어' },
        ],
        dynamicTitle: true,
      },
    },
    theme: {
      description: 'Theme',
      toolbar: {
        icon: 'contrast',
        items: [
          { value: 'light', title: 'Light' },
          { value: 'dark', title: 'Dark' },
        ],
        dynamicTitle: true,
      },
    },
    textSize: {
      description: 'Root text size',
      toolbar: {
        icon: 'zoom',
        items: ['100', '150', '200'].map((value) => ({ value, title: `${value}%` })),
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: { locale: 'en', theme: 'light', textSize: '100' },
  parameters: { layout: 'padded' },
  async beforeEach({ globals }) {
    const locale = globals.locale === 'ko' ? 'ko' : 'en';
    await setLocale(locale, { reload: false });
    document.documentElement.lang = locale;
    // The app's own fonts, as in the product: the family of the language first.
    loadFontFaces(locale);
    document.documentElement.style.fontSize = `${Number(globals.textSize) || 100}%`;
    applyTheme(globals.theme === 'dark' ? 'dark' : 'light');
  },
};

export default preview;
