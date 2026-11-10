import { mount } from 'svelte';

import './spike.css';
import '../../src/lib/editor/editor.css';

import { loadFontFaces } from '../../src/lib/fonts.ts';
import { isLocale, setLocale } from '../../src/lib/i18n/paraglide/runtime.js';

import EditorSpike from './EditorSpike.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

// The spike is also where a person tries typing Korean: draw it with the app's fonts.
loadFontFaces(document.documentElement.lang || navigator.language);

// The language of the app's own texts: English unless the address says otherwise
// (`?locale=ko`). The emoji list is searched by the names of that language too.
const locale = new URLSearchParams(location.search).get('locale');
if (locale !== null && isLocale(locale)) void setLocale(locale, { reload: false });

export default mount(EditorSpike, { target });
