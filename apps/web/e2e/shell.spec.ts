import { expect, test } from '@playwright/test';

test('the app starts and requests only hashed, compressed files', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text());
  });
  page.on('pageerror', (error) => errors.push(error.message));

  const responses: { url: string; status: number; cache: string; encoding: string }[] = [];
  page.on('response', (response) => {
    const headers = response.headers();
    responses.push({
      url: new URL(response.url()).pathname,
      status: response.status(),
      cache: headers['cache-control'] ?? '',
      encoding: headers['content-encoding'] ?? '',
    });
  });

  await page.goto('/');
  await expect(page.getByTestId('app-shell')).toBeVisible();
  // Fonts are requested while the page settles; wait for them.
  await page.waitForLoadState('networkidle');

  expect(errors).toEqual([]);
  const assets = responses.filter((response) => response.url.includes('/static/'));
  expect(assets.length).toBeGreaterThanOrEqual(2);
  for (const asset of assets) {
    expect(asset.status, asset.url).toBe(200);
    expect(asset.cache, asset.url).toContain('immutable');
    if (/\.(js|css)$/.test(asset.url)) expect(asset.encoding, asset.url).toBe('br');
  }
});

test('the page can be zoomed and follows the text size of the browser', async ({ page }) => {
  await page.goto('/');
  const viewport = await page.locator('meta[name="viewport"]').getAttribute('content');
  expect(viewport).not.toContain('user-scalable');
  expect(viewport).not.toContain('maximum-scale');

  await expect(page.getByTestId('app-shell')).toBeVisible();
  const size = (): Promise<number> =>
    page.locator('body').evaluate((element) => parseFloat(getComputedStyle(element).fontSize));
  const before = await size();
  await page.evaluate(() => {
    document.documentElement.style.fontSize = '200%';
  });
  expect(await size()).toBeCloseTo(before * 2, 1);
});

test('a deep link gets the app, and its files resolve from the base path', async ({ page }) => {
  await page.goto('/board/some-board/some-view');
  await expect(page.getByTestId('app-shell')).toBeVisible();
  await expect(page.getByRole('alert')).toHaveText('Board not found.');
});

test('an address that is not one of the app says so', async ({ page }) => {
  await page.goto('/nowhere/at/all');
  await expect(page.getByRole('alert')).toHaveText('There is nothing at this address.');
});
