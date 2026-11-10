/**
 * The settings dialog, end to end, on the in-memory backend: where it opens
 * from, its frame at each stage, and that each setting is saved at once and
 * does what it says. Runs at the three stages.
 *
 * The in-memory world is made anew on every load; of the settings only what
 * the device remembers (theme, density, the way a card opens,
 * haptics) is there again after a reload.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import { openApp, shell, showSidebar, sidebar, stageOf, strip, topBar } from './shell.support.ts';

const dialog = (page: Page): Locator => page.getByTestId('settings-dialog');
const setting = (page: Page, name: string): Locator =>
  dialog(page).locator(`[data-setting="${name}"]`);
const choice = (page: Page, name: string, label: string | RegExp): Locator =>
  setting(page, name).getByRole('radio', { name: label });

/** Opens the settings from the reader's account: on the strip where there is one, else in the sidebar. */
async function openSettings(page: Page): Promise<void> {
  if (await strip(page).isVisible()) {
    await strip(page).getByTestId('strip-open-settings').click();
  } else {
    await showSidebar(page);
    await sidebar(page).getByTestId('open-settings').click();
  }
  await expect(dialog(page)).toBeVisible();
}

async function closeSettings(page: Page): Promise<void> {
  await dialog(page).getByTestId('close-settings').click();
  await expect(dialog(page)).toBeHidden();
}

/** The settings as the backend holds them. */
function stored(page: Page): Promise<Record<string, unknown>> {
  return page.evaluate(() =>
    (
      window as unknown as {
        fvociDev: { backend: { settings: () => Promise<Record<string, unknown>> } };
      }
    ).fvociDev.backend.settings(),
  );
}

test('the settings open from the account, fill a compact screen, and close with the back button too', async ({
  page,
}) => {
  await openApp(page);
  const stage = await stageOf(page);
  const address = page.url();

  // The account is named for a screen reader by the reader's name.
  if (stage === 'medium') {
    await expect(strip(page).getByTestId('strip-open-settings')).toHaveAccessibleName(
      '김하나: account and settings',
    );
  } else {
    await showSidebar(page);
    await expect(sidebar(page).getByTestId('open-settings')).toHaveAccessibleName(
      '김하나: account and settings',
    );
  }

  await openSettings(page);
  await expect(dialog(page).getByRole('heading', { name: 'Settings' })).toBeVisible();
  // A drawer the settings were opened from is not left open behind them.
  await expect(shell(page)).not.toHaveAttribute('data-sidebar', 'drawer');

  const viewport = page.viewportSize() ?? { width: 0, height: 0 };
  const box = await dialog(page).boundingBox();
  if (stage === 'compact') {
    await expect(dialog(page)).toHaveAttribute('data-frame', 'full');
    await expect.poll(() => dialog(page).boundingBox()).toEqual({ x: 0, y: 0, ...viewport });
  } else {
    await expect(dialog(page)).toHaveAttribute('data-frame', 'dialog');
    expect(box?.width).toBeLessThan(viewport.width);
    expect(box?.height).toBeLessThanOrEqual(viewport.height - 32);
  }

  await closeSettings(page);
  await openSettings(page);
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toBeHidden();

  await openSettings(page);
  await page.goBack();
  await expect(dialog(page)).toBeHidden();
  expect(page.url()).toBe(address);
  await expect(page.getByTestId('board-page')).toBeVisible();
});

test('theme: "Dark" darkens the page at once, and a reload keeps it', async ({ page }) => {
  await openApp(page);
  const root = page.locator('html');
  await expect(root).not.toHaveClass(/\bdark\b/);

  await openSettings(page);
  await expect(choice(page, 'theme', 'Automatic')).toBeChecked();
  await choice(page, 'theme', 'Dark').click();
  await expect(root).toHaveClass(/\bdark\b/);
  await expect(choice(page, 'theme', 'Dark')).toBeChecked();
  await expect.poll(async () => (await stored(page)).theme).toBe('dark');

  await page.reload();
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(root).toHaveClass(/\bdark\b/);
  await openSettings(page);
  await expect(choice(page, 'theme', 'Dark')).toBeChecked();

  await choice(page, 'theme', 'Light').click();
  await expect(root).not.toHaveClass(/\bdark\b/);

  // "Automatic" follows the device.
  await choice(page, 'theme', 'Automatic').click();
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(root).toHaveClass(/\bdark\b/);
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(root).not.toHaveClass(/\bdark\b/);
});

test('density: "Comfortable" makes what is pressed 44px, "Compact" 32px, and a reload keeps it', async ({
  page,
  hasTouch,
}) => {
  await openApp(page);
  const undo = topBar(page).getByTestId('undo');
  const height = async (): Promise<number | undefined> => (await undo.boundingBox())?.height;
  // "Automatic" follows the pointer: roomy for a finger, compact for a mouse.
  expect(await height()).toBe(hasTouch ? 44 : 32);

  await openSettings(page);
  await choice(page, 'density', 'Comfortable').click();
  await expect(page.locator('html')).toHaveAttribute('data-density', 'comfortable');
  await expect.poll(height).toBe(44);
  // The answers of the dialog itself grow with it.
  expect(
    (await choice(page, 'density', 'Comfortable').boundingBox())?.height,
  ).toBeGreaterThanOrEqual(44);

  await page.reload();
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-density', 'comfortable');
  expect(await height()).toBe(44);

  await openSettings(page);
  await expect(choice(page, 'density', 'Comfortable')).toBeChecked();
  await choice(page, 'density', 'Compact').click();
  await expect(page.locator('html')).toHaveAttribute('data-density', 'compact');
  await expect.poll(height).toBe(32);
});

test('how a card opens: the dialog is the one answer; the other two are shown and cannot be chosen', async ({
  page,
}) => {
  await openApp(page);
  await openSettings(page);
  await expect(choice(page, 'cardOpen', 'In a dialog')).toBeChecked();
  for (const label of [/Beside the board/, /Full screen/]) {
    await expect(choice(page, 'cardOpen', label)).toBeDisabled();
    await expect(choice(page, 'cardOpen', label)).toContainText('Coming soon');
  }
  expect((await stored(page)).cardOpen).toBe('center');
});

test('haptics, the first day of the week and the way of writing hours are saved as they change', async ({
  page,
}) => {
  await openApp(page);
  await openSettings(page);

  const haptics = setting(page, 'haptics').getByRole('switch');
  await expect(haptics).toBeChecked();
  await expect(haptics).toHaveAccessibleName(/Vibration/);
  await haptics.click();
  await expect(haptics).not.toBeChecked();
  await expect.poll(async () => (await stored(page)).haptics).toBe(false);

  await choice(page, 'weekStart', 'Monday').click();
  await expect(choice(page, 'weekStart', 'Monday')).toBeChecked();
  await choice(page, 'hourCycle', '24-hour').click();
  await expect(choice(page, 'hourCycle', '24-hour')).toBeChecked();
  await expect
    .poll(async () => {
      const { weekStart, hourCycle } = await stored(page);
      return { weekStart, hourCycle };
    })
    .toEqual({ weekStart: 'monday', hourCycle: 'h23' });
});

test('time zone: picked from a list that can be searched; a refused change goes back and the reader is told', async ({
  page,
}) => {
  await openApp(page);
  await openSettings(page);
  const button = setting(page, 'timeZone').getByRole('button');
  await expect(button).toContainText('Automatic (');
  await expect(button).toHaveAccessibleName(/^Time zone Automatic/);

  await button.click();
  const search = page.getByRole('combobox', { name: 'Search time zones' });
  await search.fill('tokyo');
  await expect(page.getByRole('option')).toHaveCount(1);
  await page.getByRole('option', { name: 'Asia/Tokyo' }).click();
  await expect(button).toContainText('Asia/Tokyo');
  await expect.poll(async () => (await stored(page)).timeZone).toBe('Asia/Tokyo');
  // The settings are still open: the list closed, not the dialog.
  await expect(dialog(page)).toBeVisible();

  // The server refuses the next change: the choice shows at once, then goes back.
  await page.evaluate(() => {
    (
      window as unknown as {
        fvociDev: { backend: { failNext: (method: string, error: Error) => void } };
      }
    ).fvociDev.backend.failNext('patchSettings', new Error('refused by the test'));
  });
  await button.click();
  await page.getByRole('combobox', { name: 'Search time zones' }).fill('berlin');
  await page.getByRole('option', { name: 'Europe/Berlin' }).click();
  await expect(page.getByTestId('problems')).toContainText('The change was not saved');
  await expect(button).toContainText('Asia/Tokyo');
  expect((await stored(page)).timeZone).toBe('Asia/Tokyo');

  // "Automatic" is the first of the list.
  await button.click();
  await page.getByRole('option', { name: /^Automatic \(/ }).click();
  await expect(button).toContainText('Automatic (');
  await expect.poll(async () => (await stored(page)).timeZone).toBe('auto');
});

test('language: the page loads again in the chosen one, and the device remembers', async ({
  page,
}) => {
  await openApp(page);
  await openSettings(page);
  const button = setting(page, 'language').getByRole('button');
  await expect(button).toContainText('English');

  await button.click();
  await page.getByRole('combobox', { name: 'Search languages' }).fill('ko');
  await page.getByRole('option', { name: '한국어' }).click();

  await expect(page.locator('html')).toHaveAttribute('lang', 'ko');
  await expect(page.getByTestId('board-page')).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem('fvoci:locale'))).toBe('ko');
  await openSettings(page);
  await expect(dialog(page).getByRole('heading', { name: '설정' })).toBeVisible();
  await expect(setting(page, 'theme').getByRole('radio', { name: '어둡게' })).toBeVisible();
  await expect(setting(page, 'language').getByRole('button')).toContainText('한국어');
});

test('the layout readout is switched on and off in the settings', async ({ page }) => {
  await openApp(page);
  const readout = page.getByTestId('dev-readout');
  await expect(readout).toHaveCount(0);

  await openSettings(page);
  const toggle = setting(page, 'dev').getByRole('switch');
  await expect(toggle).not.toBeChecked();
  await toggle.click();
  await expect(toggle).toBeChecked();
  await expect(readout).toBeVisible();

  await page.reload();
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(readout).toBeVisible();

  await openSettings(page);
  await setting(page, 'dev').getByRole('switch').click();
  await expect(readout).toHaveCount(0);
});
