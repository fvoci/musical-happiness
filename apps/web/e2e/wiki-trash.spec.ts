/**
 * The trash of a wiki's pages, in the world in memory: a page deleted with
 * the pages below it comes back by undo, or from the "Deleted pages" of the
 * wiki's head (how many below it says), and the tree follows a page that
 * somebody else brings back.
 */
import { expect, test, type Page } from '@playwright/test';

import { openApp } from './shell.support.ts';
import {
  deleteBySomebodyElse,
  hideTree,
  letSomebodyElseIn,
  nameThePage,
  newWiki,
  pageAction,
  pageTree,
  restoreBySomebodyElse,
  showTree,
  treeShape,
} from './wiki.support.ts';

/** A wiki with "Welcome", "Getting started" below it and "Policies" after it. */
async function wikiWithPages(page: Page): Promise<string> {
  const wikiId = await newWiki(page, 'Handbook');
  await page.getByTestId('wiki-first-page').click();
  await nameThePage(page, 'Welcome');
  await pageAction(page, 'Welcome', 'add-child');
  await nameThePage(page, 'Getting started');
  await pageAction(page, 'Welcome', 'add-after');
  await nameThePage(page, 'Policies');
  await showTree(page);
  await expect.poll(() => treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies']);
  return wikiId;
}

async function deleteWelcome(page: Page): Promise<void> {
  await pageAction(page, 'Welcome', 'delete');
  await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Delete' }).click();
  await showTree(page);
  await expect.poll(() => treeShape(page)).toEqual(['Policies']);
}

test.describe('the trash of a wiki’s pages', () => {
  test('a page deleted comes back with those below it by undo', async ({ page }) => {
    await openApp(page);
    await wikiWithPages(page);
    await deleteWelcome(page);
    await hideTree(page);
    await page.getByTestId('undo').click();
    await showTree(page);
    await expect.poll(() => treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies']);
    await hideTree(page);
    await page.getByTestId('redo').click();
    await showTree(page);
    await expect.poll(() => treeShape(page)).toEqual(['Policies']);
  });

  test('a page deleted is listed with how many below it, and brought back', async ({ page }) => {
    await openApp(page);
    await wikiWithPages(page);
    await deleteWelcome(page);
    await page.getByTestId('open-page-trash').click();
    const trash = page.getByTestId('page-trash');
    await expect(trash.getByRole('heading')).toHaveText('Deleted pages');
    const item = trash.getByTestId('trash-items').locator('li', { hasText: 'Welcome' });
    await expect(item.getByTestId('trash-below')).toHaveText('Pages below: 1');
    await expect(item).toContainText('Deleted by');
    await item.getByRole('button', { name: 'Restore Welcome' }).click();
    await expect(trash.getByTestId('trash-empty')).toBeVisible();
    await trash.getByRole('button', { name: 'Close' }).last().click();
    await expect(trash).toBeHidden();
    await showTree(page);
    await expect.poll(() => treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies']);
  });

  test('the tree follows a page somebody else brings back', async ({ page }) => {
    await openApp(page);
    const wikiId = await wikiWithPages(page);
    await letSomebodyElseIn(page, wikiId);
    const welcome = await pageTree(page)
      .locator('[data-page][aria-label="Welcome"]')
      .getAttribute('data-page');
    if (!welcome) throw new Error('no page');
    await deleteBySomebodyElse(page, wikiId, welcome);
    await expect.poll(() => treeShape(page)).toEqual(['Policies']);
    await restoreBySomebodyElse(page, wikiId, welcome);
    await expect.poll(() => treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies']);
  });
});
