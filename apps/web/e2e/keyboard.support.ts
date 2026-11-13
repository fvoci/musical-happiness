/** Native caret navigation: macOS uses Command and an arrow for the document's ends. */
import { expect, type Locator, type Page } from '@playwright/test';

import { isApplePlatform } from '../src/lib/keys.ts';

/** Uses the browser's platform, including an Apple device emulated on Linux. */
export async function primaryKey(page: Page, key: string): Promise<string> {
  return `${await primaryName(page)}+${key}`;
}

export async function primaryName(page: Page): Promise<'Meta' | 'Control'> {
  return isApplePlatform(await page.evaluate(() => navigator.platform)) ? 'Meta' : 'Control';
}

export async function documentBoundary(page: Page, edge: 'start' | 'end'): Promise<void> {
  const key =
    process.platform === 'darwin'
      ? `Meta+${edge === 'start' ? 'ArrowUp' : 'ArrowDown'}`
      : `Control+${edge === 'start' ? 'Home' : 'End'}`;
  await page.keyboard.press(key);
}

/** Dropdowns use arrows; sheets use Tab after their opening focus has arrived. */
export async function menuTo(page: Page, target: Locator): Promise<void> {
  const dropdown = page.getByTestId('dropdown').filter({ has: target });
  if (await dropdown.count()) {
    await expect
      .poll(() => dropdown.evaluate((node) => node.contains(document.activeElement)))
      .toBe(true);
    for (let step = 0; step < 20; step += 1) {
      if (await target.evaluate((node) => node === document.activeElement)) return;
      await page.keyboard.press('ArrowDown');
    }
  } else {
    const dialog = page.getByRole('dialog').filter({ has: target });
    await expect
      .poll(() => dialog.evaluate((node) => node.contains(document.activeElement)))
      .toBe(true);
    for (let step = 0; step < 20; step += 1) {
      if (await target.evaluate((node) => node === document.activeElement)) return;
      await page.keyboard.press('Tab');
    }
  }
  await expect(target).toBeFocused();
}
