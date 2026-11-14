/**
 * Wikis, end to end, on the in-memory backend: made from "New", filed in the
 * sidebar among the boards, at an address of their own that no board's
 * screen opens (nor a board a wiki's).
 */
import { expect, test } from '@playwright/test';

import { boardPath, ON_A_BOARD, WORKSPACE } from './address.ts';
import {
  boardLink,
  boardsIn,
  FIRST_BOARD,
  openApp,
  showSidebar,
  sidebar,
  stageOf,
  strip,
  topBar,
} from './shell.support.ts';
import { goInside, newWiki, wikiPath, wikiScreen } from './wiki.support.ts';

test.describe('a new wiki', () => {
  test('is made from "New", shown empty at its address, and filed among the boards', async ({
    page,
  }) => {
    await openApp(page);
    const wikiId = await newWiki(page, 'Handbook');
    await expect(page).toHaveURL(wikiPath(wikiId));

    // Empty: the way to its first page.
    await expect(page.getByTestId('wiki-empty')).toBeVisible();
    await expect(page.getByTestId('wiki-first-page')).toHaveText('Make the first page');
    await expect(topBar(page)).toContainText('Handbook');

    await showSidebar(page);
    const link = sidebar(page).getByRole('link', { name: 'Handbook, wiki', exact: true });
    await expect(link).toHaveAttribute('aria-current', 'page');
    await expect(link.locator('[data-board-icon]')).toHaveAttribute('data-kind', 'wiki');
    // The default category holds boards and wikis, under a name of neither.
    expect(await boardsIn(page, 'General')).toContain('Handbook');
    await expect(
      sidebar(page).getByRole('button', { name: 'Menu of the wiki Handbook' }),
    ).toBeVisible();
    if ((await stageOf(page)) === 'medium') {
      await expect(strip(page).getByRole('link', { name: 'Handbook, wiki' })).toBeVisible();
    }
  });

  test('is opened from the sidebar, and a board from there again', async ({ page }) => {
    await openApp(page);
    const wikiId = await newWiki(page, 'Handbook');
    await showSidebar(page);
    await boardLink(page, FIRST_BOARD).click();
    await expect(page).toHaveURL(ON_A_BOARD);
    await expect(page.getByTestId('board-page')).toBeVisible();
    await showSidebar(page);
    await sidebar(page).getByRole('link', { name: 'Handbook, wiki' }).click();
    await expect(page).toHaveURL(wikiPath(wikiId));
    await expect(wikiScreen(page)).toBeVisible();
  });

  test('is no board: not at a board’s address, nor among the templates; a board is no wiki', async ({
    page,
  }) => {
    await openApp(page);
    const boardId = new URL(page.url()).pathname.split('/')[4] ?? '';
    const wikiId = await newWiki(page, 'Handbook');

    await goInside(page, boardPath(wikiId));
    await expect(page.getByTestId('not-found')).toBeVisible();
    await expect(page.getByTestId('board-page')).toHaveCount(0);

    await goInside(page, wikiPath(boardId));
    await expect(page.getByTestId('not-found')).toBeVisible();
    await expect(wikiScreen(page)).toHaveCount(0);

    await goInside(page, boardPath(boardId));
    await expect(page.getByTestId('board-page')).toBeVisible();
    await showSidebar(page);
    await sidebar(page).getByTestId('add-board').click();
    const picker = page.getByTestId('template-picker');
    await expect(picker.getByTestId('templates')).toBeAttached();
    await expect(picker.getByText('Handbook')).toHaveCount(0);
  });

  test('a wiki’s address without a wiki is not found, and the root goes to the last place', async ({
    page,
  }) => {
    await openApp(page);
    const wikiId = await newWiki(page, 'Handbook');
    // The workspace's address alone goes back to where the reader was: the wiki.
    await goInside(page, `/w/${WORKSPACE}`);
    await expect(page).toHaveURL(wikiPath(wikiId));
    await goInside(page, wikiPath('01930000-0000-7000-8000-00000000ffff'));
    await expect(page.getByTestId('not-found')).toBeVisible();
  });
});
