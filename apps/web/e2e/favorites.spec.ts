/**
 * The reader's stars, on the backend in memory: the ★ of a board at the top
 * bar and of a card at its head put them under "Favorites" at the head of the
 * sidebar, where the keyboard moves them as it moves boards; the search, before
 * anything is typed, offers them and what was opened last on this browser, of
 * which no diary is kept; and the stars are buttons of 44 pixels a side.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import { ON_A_CARD } from './address.ts';
import { openApp, showSidebar, sidebar, topBar } from './shell.support.ts';

const favorites = (page: Page): Locator => sidebar(page).getByTestId('sidebar-favorites');
const starred = (page: Page): Locator => favorites(page).locator('[data-favorite]');

async function titlesOf(list: Locator): Promise<string[]> {
  return (await list.allTextContents()).map((text) => text.trim());
}

test('a board and a card are starred, listed in the sidebar, and moved by the keyboard', async ({
  page,
}) => {
  await openApp(page);
  const boardStar = topBar(page).getByTestId('favorite-toggle');
  await expect(boardStar).toHaveAccessibleName('Favorite');
  await expect(boardStar).toHaveAttribute('aria-pressed', 'false');
  const box = await boardStar.boundingBox();
  expect(box && [Math.round(box.width), Math.round(box.height)]).toEqual([44, 44]);
  await boardStar.click();
  await expect(boardStar).toHaveAttribute('aria-pressed', 'true');

  await page.locator('[data-card]').first().click();
  await expect(page).toHaveURL(ON_A_CARD);
  const cardStar = page.getByTestId('card-header').getByTestId('favorite-toggle');
  await cardStar.click();
  await expect(cardStar).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.press('Escape');

  await showSidebar(page);
  await expect(starred(page)).toHaveCount(2);
  const [boardName] = await titlesOf(starred(page));
  expect(boardName).toBeTruthy();

  // The card is carried above the board with the keyboard, as a board is among boards.
  await starred(page).nth(1).focus();
  await page.keyboard.press('Space');
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('Space');
  await expect.poll(async () => (await titlesOf(starred(page)))[1]).toBe(boardName);

  // Folded by hand, the stars are out of the way.
  await favorites(page).getByRole('button', { name: 'Favorites' }).click();
  await expect(starred(page)).toHaveCount(0);
});

test('the search starts with the stars and what was opened last, and goes there', async ({
  page,
}) => {
  await openApp(page);
  await topBar(page).getByTestId('favorite-toggle').click();
  await page.locator('[data-card]').first().click();
  await expect(page).toHaveURL(ON_A_CARD);
  await page.keyboard.press('Escape');

  await topBar(page).getByTestId('open-search').click();
  const dialog = page.getByTestId('search-dialog');
  await expect(dialog.getByTestId('search-favorites').locator('[data-search-place]')).toHaveCount(
    1,
  );
  const recent = dialog.getByTestId('search-recent').locator('[data-search-place]');
  await expect(recent.first()).toBeVisible();
  const kept = await page.evaluate(() =>
    Object.keys(localStorage).filter((key) => key.startsWith('fvoci:recent:')),
  );
  expect(kept).toHaveLength(1);

  // Typed, the stars whose title holds it, and no more what was opened last.
  await page.getByTestId('search-field').fill('zzzz-nothing');
  await expect(dialog.getByTestId('search-favorites')).toHaveCount(0);
  await expect(dialog.getByTestId('search-recent')).toHaveCount(0);
  await page.getByTestId('search-field').fill('');
  await recent.first().click();
  await expect(dialog).toHaveCount(0);
});
