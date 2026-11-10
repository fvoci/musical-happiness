import { mount } from 'svelte';

import '../styles/app.css';
import { watchDisplay } from '$lib/display.svelte.ts';
import { loadFontFaces } from '$lib/fonts.js';
import { watchKeyboard } from '$lib/layout/keyboard.ts';
import { insets, page } from '$lib/layout/room.svelte.ts';
import { watchSystemTheme } from '$lib/theme.svelte.js';

import App from './App.svelte';
import { startLocale } from './locale.ts';
import { readBasePath, Router } from './router.svelte.ts';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

const locale = startLocale();
watchSystemTheme();
watchDisplay();
watchKeyboard();
insets.watch();
// The page is what is inside the edges the device keeps: #app has them as padding.
page.attach(target);
loadFontFaces(locale);

const router = new Router(readBasePath(document));
router.listen();

export default mount(App, { target, props: { router } });
