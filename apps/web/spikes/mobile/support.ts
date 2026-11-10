/** What the two browser test files of this spike share. */
import type { Page } from '@playwright/test';

export const PAGE = '/spikes/mobile/';

/** The phone and the desktop of the proposals, in CSS pixels. */
export const PHONE = { width: 360, height: 740 };
export const DESKTOP = { width: 1280, height: 800 };

/**
 * The moment every page believes it is: the calendar marks "today", and a
 * picture must not change from one day to the next.
 */
export const NOW = new Date('2026-11-10T09:00:00+09:00');

/**
 * Waits until the page is what a person would see: the Korean font has
 * arrived (its rules are requested after the first paint, so `fonts.ready`
 * alone can be too early) and nothing is still sliding or fading in.
 */
export async function settled(page: Page): Promise<void> {
  await page.waitForFunction(() =>
    [...document.fonts].some(
      (face) => face.family.includes('Noto Sans KR') && face.status === 'loaded',
    ),
  );
  await page.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all(
      document.getAnimations().map((animation) => animation.finished.catch(() => undefined)),
    );
    await new Promise((resolve) => requestAnimationFrame(resolve));
  });
}
