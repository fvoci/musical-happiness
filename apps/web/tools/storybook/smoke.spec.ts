import { readFileSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

interface Entry {
  id: string;
  type: string;
}

const index = JSON.parse(
  readFileSync(new URL('../../storybook-static/index.json', import.meta.url), 'utf8'),
) as { entries: Record<string, Entry> };
const stories = Object.values(index.entries).filter((entry) => entry.type === 'story');
if (stories.length === 0) throw new Error('The built Storybook contains no stories');

function browserErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text());
  });
  page.on('pageerror', (error) => errors.push(error.message));
  return errors;
}

for (const story of stories) {
  test(story.id, async ({ page }, info) => {
    const errors = browserErrors(page);
    const korean = info.project.name === 'ko-dark-390';
    const globals = korean ? 'locale:ko;theme:dark' : 'locale:en;theme:light';
    await page.goto(`iframe.html?id=${story.id}&viewMode=story&globals=${globals}`);
    await expect(page.locator('html')).toHaveAttribute('data-story-finished', story.id);
    await expect(page.locator('html')).toHaveAttribute('data-story-result', 'success');
    await expect(page.locator('html')).toHaveAttribute('lang', korean ? 'ko' : 'en');
    await expect(page.locator('html')).toHaveClass(korean ? /dark/ : /^(?!.*\bdark\b)/);
    await expect(page.locator('#storybook-root')).not.toBeEmpty();
    await expect(page.locator('body')).not.toHaveClass(/sb-show-errordisplay/);
    await page.waitForLoadState('networkidle');
    await page.evaluate(() => document.fonts.ready);
    await expect(page.locator('.doc-diagram[data-state="pending"]')).toHaveCount(0);
    expect(errors, `${story.id}: browser errors`).toEqual([]);
  });
}

test('the guard observes console errors and uncaught exceptions', async ({ page }) => {
  const errors = browserErrors(page);
  await page.goto('about:blank');
  await page.evaluate(() => {
    console.error('example console failure');
    setTimeout(() => {
      throw new Error('example uncaught failure');
    }, 0);
  });
  await expect.poll(() => errors).toEqual(['example console failure', 'example uncaught failure']);
});

test('the language toolbar refreshes a mounted story', async ({ page }) => {
  const errors = browserErrors(page);
  await page.goto(
    'iframe.html?id=activity-audit-list--every-action&viewMode=story&globals=locale:en;theme:light',
  );
  await expect(page.locator('html')).toHaveAttribute('data-story-result', 'success');
  const before = await page.getByTestId('audit-list').innerText();
  // The same public channel event the manager toolbar sends to the iframe.
  await page.evaluate(() => {
    const channel = (
      window as unknown as {
        __STORYBOOK_ADDONS_CHANNEL__: { emit: (event: string, payload: unknown) => void };
      }
    ).__STORYBOOK_ADDONS_CHANNEL__;
    channel.emit('updateGlobals', { globals: { locale: 'ko', theme: 'dark', textSize: '150' } });
  });
  await expect(page.locator('html')).toHaveAttribute('lang', 'ko');
  await expect(page.locator('html')).toHaveClass(/dark/);
  await expect(page.locator('html')).toHaveCSS('font-size', '24px');
  await expect(page.getByTestId('audit-list')).not.toHaveText(before);
  await expect(page.getByTestId('audit-list')).toContainText('워크스페이스');
  expect(errors).toEqual([]);
});

test.describe('a reader in another time zone', () => {
  test.use({ timezoneId: 'America/Los_Angeles' });
  test('the audit example keeps its stated time zone', async ({ page }) => {
    await page.goto(
      'iframe.html?id=activity-audit-list--every-action&viewMode=story&globals=locale:en',
    );
    await expect(page.locator('html')).toHaveAttribute('data-story-result', 'success');
    await expect(page.locator('time').first()).toHaveText('Nov 13, 2026, 12:00 PM');
  });
});
