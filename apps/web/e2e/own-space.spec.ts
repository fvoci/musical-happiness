/**
 * The reader's own space, end to end, on the in-memory backend: the switcher
 * puts it first, set apart and named "My space", and says which workspace is
 * shown; the app opens on the shared workspace all the same. Entered, it
 * starts empty and says what it is; what is made there stays there, and
 * nothing of the shared workspace shows in it.
 *
 * What is hidden in the own space (people, invitations, the organization)
 * comes from the server alone, so it is tested against the server
 * (`e2e-live/own-space.spec.ts`).
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import { ON_A_VIEW, placeOf, WORKSPACE } from './address.ts';
import { boardLink, FIRST_BOARD, openApp, showSidebar, sidebar } from './shell.support.ts';
import { ON_A_WIKI } from './wiki.support.ts';

const switcher = (page: Page): Locator => sidebar(page).getByTestId('workspace-switcher');
const workspacesMenu = (page: Page): Locator =>
  page.getByRole('menu', { name: 'Switch workspace', exact: true });

/** Opens the switcher and goes to the workspace of that name. */
async function switchTo(page: Page, name: string): Promise<void> {
  await showSidebar(page);
  await switcher(page).click();
  await workspacesMenu(page).getByRole('menuitem', { name, exact: true }).click();
}

test('the switcher puts the own space first and says which is shown', async ({ page }) => {
  await openApp(page);
  // The app opens on the workspace the reader shares with others.
  expect(placeOf(page.url()).workspaceId).toBe(WORKSPACE);
  await showSidebar(page);
  await expect(switcher(page)).toHaveAccessibleName('Switch workspace: fvoci');
  await expect(switcher(page)).not.toHaveAttribute('data-own-space');

  await switcher(page).click();
  const items = workspacesMenu(page).getByRole('menuitem');
  await expect(items).toHaveText(['My space', 'fvoci']);
  await expect(items.nth(0)).toHaveAccessibleName('My space');
  await expect(items.nth(1)).toHaveAccessibleName('fvoci, current');
  // Its mark is not that of the others.
  const markOf = (item: Locator): Promise<string | null> =>
    item.locator('svg').first().getAttribute('class');
  expect(await markOf(items.nth(0))).not.toBe(await markOf(items.nth(1)));

  await items.nth(0).click();
  await expect(page).toHaveURL(/\/w\/[0-9a-f-]{36}$/u);
  expect(placeOf(page.url()).workspaceId).not.toBe(WORKSPACE);
  await showSidebar(page);
  await expect(switcher(page)).toHaveAccessibleName('Switch workspace: My space');
  await expect(switcher(page)).toHaveAttribute('data-own-space', '');
});

test('the own space starts empty, keeps what is made in it, and shows nothing of the shared one', async ({
  page,
}) => {
  await openApp(page);
  await switchTo(page, 'My space');
  const start = page.getByTestId('workspace-start');
  await expect(start.getByRole('heading', { name: 'My space' })).toBeVisible();
  await expect(start).toContainText('yours alone');
  await showSidebar(page);
  await expect(boardLink(page, FIRST_BOARD)).toHaveCount(0);
  await expect(sidebar(page).getByRole('link')).toHaveCount(0);

  // The start leads to making a board, as "Add board or wiki" does.
  const own = placeOf(page.url()).workspaceId;
  await page.keyboard.press('Escape');
  await start.getByTestId('workspace-start-add').click();
  await page.getByTestId('template-picker').getByTestId('empty-board').click();
  await expect(page).toHaveURL(ON_A_VIEW);
  expect(placeOf(page.url()).workspaceId).toBe(own);
  await expect(page.getByTestId('board-page')).toBeVisible();
  // Nobody else is there to find a wiki: it is not asked.
  await showSidebar(page);
  await sidebar(page).getByTestId('add-board').click();
  await page.getByTestId('template-picker').getByTestId('new-wiki').click();
  const naming = page.getByTestId('new-wiki-dialog');
  await expect(naming.getByRole('radio')).toHaveCount(0);
  await naming.getByTestId('new-wiki-title').fill('Notes of mine');
  await naming.getByTestId('new-wiki-create').click();
  await expect(page).toHaveURL(ON_A_WIKI);
  await showSidebar(page);
  await expect(sidebar(page).getByRole('link')).toHaveCount(2);

  await switchTo(page, 'fvoci');
  await expect(page.getByTestId('board-page')).toBeVisible();
  expect(placeOf(page.url()).workspaceId).toBe(WORKSPACE);
  await showSidebar(page);
  await expect(boardLink(page, FIRST_BOARD)).toBeVisible();
  await expect(sidebar(page).getByText('Notes of mine')).toHaveCount(0);
  await expect(boardLink(page, '(Untitled Board)')).toHaveCount(0);

  // Back in the own space, it is as it was left.
  await switchTo(page, 'My space');
  await showSidebar(page);
  await expect(sidebar(page).getByRole('link')).toHaveCount(2);
  await expect(sidebar(page).getByText('Notes of mine')).toBeVisible();
});

test('a board of the own space is shared by its public link alone, with nobody to count', async ({
  page,
}) => {
  await openApp(page);
  // In the shared workspace the board has its members and the whole dialog.
  await expect(page.getByTestId('board-people')).toBeVisible();
  await page.getByTestId('share-board').click();
  const dialog = page.getByTestId('share-dialog');
  await expect(dialog.getByTestId('share-open')).toBeVisible();
  await expect(dialog.getByTestId('share-members')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  await switchTo(page, 'My space');
  await page.keyboard.press('Escape');
  await page.getByTestId('workspace-start-add').click();
  await page.getByTestId('template-picker').getByTestId('empty-board').click();
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page.getByTestId('board-people')).toHaveCount(0);
  await page.getByTestId('share-board').click();
  await expect(dialog.getByTestId('share-public-section')).toBeVisible();
  await expect(dialog.getByTestId('share-open')).toHaveCount(0);
  await expect(dialog.getByTestId('share-members')).toHaveCount(0);
  await expect(dialog.getByTestId('share-search')).toHaveCount(0);
  await dialog.getByTestId('share-public').click();
  await expect(dialog.getByTestId('share-link')).toHaveValue(/\/shared\/[0-9a-f-]{36}\?r=[^&]+$/u);
});
