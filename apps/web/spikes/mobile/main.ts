import { mount } from 'svelte';

import './mobile.css';
import { loadFontFaces } from '$lib/fonts.js';
import { applyTheme, watchSystemTheme } from '$lib/theme.svelte.js';

import App from './App.svelte';
import { CURRENT } from './current.ts';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

const root = document.documentElement;
if (CURRENT.fakeSafeArea !== 'none') root.dataset.fakeSafe = CURRENT.fakeSafeArea;
if (CURRENT.fakeKeyboard) root.dataset.fakeKeyboard = '';

watchSystemTheme();
if (CURRENT.theme) applyTheme(CURRENT.theme);
loadFontFaces(root.lang);

export default mount(App, { target });
