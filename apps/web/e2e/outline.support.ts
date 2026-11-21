/**
 * What the tests of a wiki page's outline share: a long body put into the
 * page as a paste, the page's scrolling column, and where a heading of the
 * body lies in it.
 */
import type { Locator, Page } from '@playwright/test';

import { pageBody } from './wiki.support.ts';

/** The words of each section of `longBody`: some lines, so that a section is taller than a quarter of the page. */
export const SECTION_WORDS = `Words of the section. ${'More words of it. '.repeat(40).trim()}`;

/** A body of `count` headings of levels 1, 2, 3, 2 in turn, each with a paragraph of `words` below it. */
export function longBody(count: number, words = SECTION_WORDS): string {
  return Array.from({ length: count }, (_, at) => {
    const level = [1, 2, 3, 2][at % 4] ?? 2;
    return `<h${level}>Section ${at + 1}</h${level}><p>${words}</p>`;
  }).join('');
}

/** Pastes `html` into the page's body where its caret is, as a browser reports a paste. */
export async function pasteIntoBody(page: Page, html: string): Promise<void> {
  const body = pageBody(page);
  await body.click();
  await body.evaluate((element, pasted) => {
    const data = new DataTransfer();
    data.setData('text/html', pasted);
    data.setData('text/plain', pasted.replace(/<[^>]+>/g, ' '));
    element.dispatchEvent(
      new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
    );
  }, html);
}

/** The outline beside the page. */
export const outlineBeside = (page: Page): Locator => page.getByTestId('page-outline-beside');

/** The outline wherever it is: beside the page, or the list behind its button. */
export const outline = (page: Page): Locator =>
  page.getByRole('navigation', { name: /^(Contents|목차)$/ });

/** The heading of the page's body that has these words. */
export const bodyHeading = (page: Page, words: string): Locator =>
  page
    .getByTestId('page-body')
    .locator('h1, h2, h3')
    .filter({ hasText: new RegExp(`^${words}$`) });

/** The element the page scrolls in: the one holding the page's view. */
export const pageColumn = (page: Page): Locator =>
  page.locator('[data-testid="wiki-screen"] .overflow-y-auto:has([data-testid="page-view"])');

/** How far the top of a heading is below the top of the page's column, in pixels. */
export async function headingBelowTop(page: Page, words: string, column = pageColumn(page)) {
  const [heading, box] = await Promise.all([
    bodyHeading(page, words).boundingBox(),
    column.boundingBox(),
  ]);
  if (!heading || !box) throw new Error(`no heading ${words} in sight`);
  return heading.y - box.y;
}

/** The headings of the outline, as items to choose. */
export const outlineItems = (page: Page): Locator => outline(page).locator('li button');

/** The item of the outline marked as the one being read. */
export const currentItem = (page: Page): Locator =>
  outline(page).locator('li button[aria-current="location"]');
