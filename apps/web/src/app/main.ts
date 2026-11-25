import { mount } from 'svelte';

import '../styles/app.css';
import { noteChunkFailure } from '$lib/chunk-reload.svelte.ts';
import { watchDisplay } from '$lib/display.svelte.ts';
import { startCodeLines } from '$lib/editor/code-lines.svelte.ts';
import { loadFontFaces } from '$lib/fonts.js';
import { watchKeyboard } from '$lib/layout/keyboard.ts';
import { insets, page } from '$lib/layout/room.svelte.ts';
import { watchSystemTheme } from '$lib/theme.svelte.js';

import { startLocale } from './locale.ts';
import { isSharedRoute, readBasePath, Router } from './router.svelte.ts';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

const locale = startLocale();
watchSystemTheme();
watchDisplay();
startCodeLines();
watchKeyboard((height) => {
  insets.keyboard = height;
});
insets.watch();
// The page is what is inside the edges the device keeps: #app has them as padding.
page.attach(target);
loadFontFaces(locale);

const router = new Router(readBasePath(document));
router.listen();

async function mountEntry(appTarget: HTMLElement): Promise<Record<string, unknown>> {
  try {
    if (router.route.kind === 'passwordReset' || router.route.kind === 'passwordForgot') {
      const { default: ResetPasswordApp } = await import('./ResetPasswordApp.svelte');
      return mount(ResetPasswordApp, { target: appTarget, props: { router } });
    }
    if (router.route.kind === 'emailVerify') {
      const { default: VerifyEmailApp } = await import('./VerifyEmailApp.svelte');
      return mount(VerifyEmailApp, { target: appTarget, props: { router } });
    }
    if (router.route.kind === 'emailConfirm') {
      const { default: ConfirmEmailApp } = await import('./ConfirmEmailApp.svelte');
      return mount(ConfirmEmailApp, { target: appTarget, props: { router } });
    }
    if (router.route.kind === 'emailRevert') {
      const { default: RevertEmailApp } = await import('./RevertEmailApp.svelte');
      return mount(RevertEmailApp, { target: appTarget, props: { router } });
    }
    if (isSharedRoute(router.route)) {
      const { default: SharedApp } = await import('./SharedApp.svelte');
      return mount(SharedApp, { target: appTarget, props: { router } });
    }
    const { default: App } = await import('./App.svelte');
    return mount(App, { target: appTarget, props: { router } });
  } catch (error) {
    void noteChunkFailure();
    throw error;
  }
}

export default mountEntry(target);
