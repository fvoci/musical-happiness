import { mount } from 'svelte';

import '../styles/app.css';
import { loadFontFaces } from '$lib/fonts.js';
import { watchSystemTheme } from '$lib/theme.svelte.js';

import App from './App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

watchSystemTheme();
loadFontFaces(document.documentElement.lang || navigator.language);

export default mount(App, { target });
