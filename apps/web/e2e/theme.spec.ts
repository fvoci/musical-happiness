/**
 * The brightness is on the page before the app's script has run: a reader
 * who chose the dark screen never sees a light one first. The script at the
 * top of index.html does it from what the device remembers.
 */
import { expect, test, type Page } from '@playwright/test';

/** What the root element says, with the app's own script kept from loading. */
async function beforeTheApp(
  page: Page,
): Promise<{ dark: boolean; scheme: string; bar: string; density: string }> {
  await page.route(/\/static\/index-[0-9a-f]+\.js$/, (route) => route.abort());
  await page.goto('/');
  return page.evaluate(() => ({
    dark: document.documentElement.classList.contains('dark'),
    scheme: document.documentElement.style.colorScheme,
    bar: document.querySelector('meta[name="theme-color"]')?.getAttribute('content') ?? '',
    density: document.documentElement.dataset.density ?? '',
  }));
}

const remember = (page: Page, settings: object): Promise<void> =>
  page.addInitScript((stored) => {
    localStorage.setItem('fvoci:display', stored);
  }, JSON.stringify(settings));

test('a remembered dark screen is dark before the app starts', async ({ page }) => {
  await remember(page, { brightness: 'dark' });
  expect(await beforeTheApp(page)).toMatchObject({ dark: true, scheme: 'dark', bar: '#0a0a0a' });
});

test('a remembered light screen stays light on a device set to dark', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await remember(page, { brightness: 'light' });
  expect(await beforeTheApp(page)).toMatchObject({ dark: false, scheme: 'light', bar: '#ffffff' });
});

test('with nothing remembered the device decides', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  expect((await beforeTheApp(page)).dark).toBe(true);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.reload();
  expect(await page.evaluate(() => document.documentElement.classList.contains('dark'))).toBe(
    false,
  );
});

test('the density follows the pointer until the reader fixes it', async ({ page, hasTouch }) => {
  expect((await beforeTheApp(page)).density).toBe(hasTouch ? 'comfortable' : 'compact');
});

test('a remembered density is used whatever the pointer is', async ({ page }) => {
  await remember(page, { density: 'comfortable' });
  expect((await beforeTheApp(page)).density).toBe('comfortable');
});

test('something else under the key does not stop the page', async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem('fvoci:display', 'not json');
  });
  expect(await beforeTheApp(page)).toMatchObject({ dark: false, bar: '#ffffff' });
});

test('once the app runs, the account decides, and what it says is remembered for next time', async ({
  page,
}) => {
  // The account (kept between visits by the in-memory backend) says dark.
  await page.addInitScript(() => {
    localStorage.setItem('fvoci:memory-settings', JSON.stringify({ brightness: 'dark' }));
  });
  await page.goto('/');
  await expect(page.getByTestId('app-shell')).toBeVisible();
  await expect(page.locator('html')).toHaveClass(/dark/);
  const background = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(background).not.toBe('rgb(255, 255, 255)');
  // And the device now remembers it for the script that runs before the app.
  const remembered = await page.evaluate(() => localStorage.getItem('fvoci:display'));
  expect(JSON.parse(remembered ?? '{}')).toMatchObject({ brightness: 'dark' });
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute('content', '#0a0a0a');
});
