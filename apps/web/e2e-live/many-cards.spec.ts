/**
 * A board of more cards than one page of the server holds (1000): the page
 * reads them page after page, and every card is on the board.
 */
import { expect, test } from '@playwright/test';

import { boardPath, makeBoard, person, register, signedIn, watchErrors } from './support.ts';

test.use({ locale: 'en-US' });

/** One more than the most cards a page of the server holds. */
const MANY = 1001;

/** How many cards one request makes. */
const AT_ONCE = 250;

test('every card of a board of more than a thousand is shown', async ({ page, baseURL }) => {
  test.setTimeout(120_000);
  const base = baseURL ?? '';
  const owner = person('many');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, { title: 'Many' });
  const doing = board.options.Doing;
  for (let from = 0; from < MANY; from += AT_ONCE) {
    const count = Math.min(AT_ONCE, MANY - from);
    const data = Array.from({ length: count }, (_, at) => ({
      containerId: board.boardId,
      type: 'card',
      title: `Card ${String(from + at + 1)}`,
      createAt: 1,
      updateAt: 1,
      fields: { properties: { status: doing } },
    }));
    const made = await api.post(`/api/v1/w/${board.workspaceId}/boards/${board.boardId}/blocks`, {
      data,
    });
    expect(made.ok(), `cards from ${String(from)}: ${await made.text()}`).toBe(true);
  }

  // The page is signed in as the client of the API is. Signing in on the page is no part of this,
  // and the shell is drawn in one breath with the cards: a wait for it is a wait for a thousand
  // cards, which are the work of seconds and of more on a busy machine.
  await page.context().addCookies((await api.storageState()).cookies);
  const errors = watchErrors(page);
  const lastPage = page.waitForResponse((answer) => answer.url().includes('/cards?page=1&'));
  await page.goto(boardPath(board, board.viewId));
  expect((await lastPage).ok(), 'the second page of cards').toBe(true);
  await expect(page.getByTestId('board-page')).toBeVisible();
  const column = page.locator('[data-column]').filter({
    has: page.locator('h3').filter({ hasText: /^\s*Doing\s*$/ }),
  });
  await expect(column.locator('[data-column-count] [aria-hidden="true"]')).toHaveText(String(MANY));
  expect(errors).toEqual([]);
});
