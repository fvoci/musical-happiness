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
