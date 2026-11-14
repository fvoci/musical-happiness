/**
 * What the tests of the wikis share: making one through "New", its address,
 * and where its parts are on the page.
 */
import { expect, type Locator, type Page } from '@playwright/test';

import { WORKSPACE } from './address.ts';
import { showSidebar, sidebar } from './shell.support.ts';

const ID = '[0-9a-f-]{36}';

/** A wiki of the world in memory, and a page of it. */
export function wikiPath(wikiId: string, pageId?: string): string {
  return `/w/${WORKSPACE}/wikis/${wikiId}${pageId ? `/${pageId}` : ''}`;
}

/** Any wiki's address, with or without a page. */
export const ON_A_WIKI = new RegExp(`/w/${ID}/wikis/(${ID})(/(${ID}))?$`);
/** Any page of any wiki. */
export const ON_A_PAGE = new RegExp(`/w/${ID}/wikis/(${ID})/(${ID})$`);

/** The wiki and the page an address names; empty strings for what it does not. */
export function wikiPlaceOf(address: string): { wikiId: string; pageId: string } {
  const found = ON_A_WIKI.exec(new URL(address, 'http://here').pathname);
  return { wikiId: found?.[1] ?? '', pageId: found?.[3] ?? '' };
}

export const wikiScreen = (page: Page): Locator => page.getByTestId('wiki-screen');

/** Makes a wiki through "New" and waits for its page; gives its identifier. */
export async function newWiki(page: Page, title: string, open = true): Promise<string> {
  await showSidebar(page);
  await sidebar(page).getByTestId('add-board').click();
  await page.getByTestId('template-picker').getByTestId('new-wiki').click();
  const dialog = page.getByTestId('new-wiki-dialog');
  await dialog.getByTestId('new-wiki-title').fill(title);
  await dialog.getByTestId(open ? 'new-wiki-open' : 'new-wiki-private').check();
  await dialog.getByTestId('new-wiki-create').click();
  await expect(page).toHaveURL(ON_A_WIKI);
  await expect(wikiScreen(page)).toBeVisible();
  return wikiPlaceOf(page.url()).wikiId;
}

/**
 * Goes to an address inside the page, as a link of the app would: the world
 * in memory starts anew on a load, and with it whatever a test made.
 */
export async function goInside(page: Page, path: string): Promise<void> {
  await page.evaluate((address) => {
    history.pushState(null, '', address);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }, path);
}

/** The tree of pages: its items as drawn, each with its level and name. */
export const pageTree = (page: Page): Locator => page.getByTestId('page-tree');

export async function treeShape(page: Page): Promise<string[]> {
  return pageTree(page)
    .getByRole('treeitem')
    .evaluateAll((items) =>
      items.map(
        (item) =>
          `${'  '.repeat(Number(item.getAttribute('aria-level')) - 1)}${item.getAttribute('aria-label') ?? ''}`,
      ),
    );
}

/** A page of the tree by its name. */
export const treeItem = (page: Page, name: string): Locator =>
  pageTree(page).getByRole('treeitem', { name, exact: true });

/** Gives the page shown its title, as typed, and leaves the field. */
export async function nameThePage(page: Page, title: string): Promise<void> {
  const field = page.getByTestId('page-title');
  await expect(field).toBeFocused();
  await page.keyboard.type(title);
  await page.keyboard.press('Enter');
  // In the tree, which may be a drawer that is away (inert, so out of the roles' reach).
  await expect(pageTree(page).locator(`[role="treeitem"][aria-label="${title}"]`)).toBeAttached();
}

/** Brings the tree into view: nothing to do in a wide pane, the drawer in a narrow one. */
export async function showTree(page: Page): Promise<void> {
  const opener = page.getByTestId('open-page-tree');
  if (await opener.isVisible()) await opener.click();
  await expect(pageTree(page)).toBeVisible();
}

/** Puts the tree's drawer away, where it is one: Escape, as a keyboard would. */
export async function hideTree(page: Page): Promise<void> {
  const drawer = page.getByRole('dialog', { name: 'Pages' });
  if (!(await drawer.isVisible())) return;
  await page.keyboard.press('Escape');
  await expect(drawer).toBeHidden();
}

/** Opens a page's menu in the tree and presses one of its lines. */
export async function pageAction(page: Page, name: string, action: string): Promise<void> {
  await showTree(page);
  await pageTree(page)
    .getByRole('button', { name: `Menu of the page ${name}` })
    .click();
  await page.locator(`[data-action="${action}"]`).click();
}
