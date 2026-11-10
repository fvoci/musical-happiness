/** What the two browser test files of this spike share. */
import { expect, type Page } from '@playwright/test';

import { settled } from '../mobile/support.ts';

import { SIZES, addressOf, type ScreenId, type Variant } from './params.ts';

export { settled };

export const PAGE = '/spikes/calendar/';

/** The phone and the desktop of the proposals, in CSS pixels. */
export const PHONE = SIZES[360];
export const DESKTOP = SIZES[1280];

/**
 * The moment the browser believes it is. The calendar itself takes its
 * "now" from the made-up data (the same moment); this is for what asks the
 * browser, such as the generated date picker.
 */
export const NOW = new Date('2026-11-10T09:00:00+09:00');

/** Opens one mock-up and waits until it is what a person would see. */
export async function show(
  page: Page,
  screen: ScreenId,
  variant: Variant,
  extra = '',
): Promise<void> {
  await page.goto(PAGE + addressOf(screen, variant, extra));
  await expect(page.getByTestId('calendar-toolbar')).toBeVisible();
  await settled(page);
}
