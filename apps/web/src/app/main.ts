import { mount } from 'svelte';

import '../styles/app.css';
import { watchDisplay } from '$lib/display.svelte.ts';
import { loadFontFaces } from '$lib/fonts.js';
import { watchKeyboard } from '$lib/layout/keyboard.ts';
import { insets, page } from '$lib/layout/room.svelte.ts';
import { watchSystemTheme } from '$lib/theme.svelte.js';

import App from './App.svelte';
import ResetPasswordApp from './ResetPasswordApp.svelte';
import SharedApp from './SharedApp.svelte';
import { startLocale } from './locale.ts';
import { readBasePath, Router } from './router.svelte.ts';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

const locale = startLocale();
watchSystemTheme();
watchDisplay();
watchKeyboard((height) => {
  insets.keyboard = height;
});
insets.watch();
// The page is what is inside the edges the device keeps: #app has them as padding.
page.attach(target);
loadFontFaces(locale);

const router = new Router(readBasePath(document));
router.listen();

// A board's public link is read without the app around it, signed in or not;
// so is an address under `/shared` that is no whole link (not found there).
export default router.route.passwordReset || router.route.passwordForgot
  ? mount(ResetPasswordApp, { target, props: { router } })
  : router.route.shared
    ? mount(SharedApp, { target, props: { router } })
    : mount(App, { target, props: { router } });
