/**
 * Cards on the real server, on a board's kanban: making them (and, as the
 * work goes on, changing and moving them). Each test makes its own board through the API as its owner, who
 * then works on the page.
 */
import { expect, test, type Page } from '@playwright/test';

import {
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US' });

function column(page: Page, name: string) {
  return page.locator('[data-column]').filter({
    has: page.locator('h3').filter({ hasText: new RegExp(`^\\s*${name}\\s*$`) }),
  });
}

test('a card made in a column is the server’s, with the value of that column', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('maker');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, { title: 'Errands' });

  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${board.viewId}`);
  await signInOnPage(page, owner);
  await expect(page.getByTestId('board-page')).toBeVisible();

  await page.getByRole('button', { name: 'New card in Doing' }).click();
  const dialog = page.getByTestId('card-dialog');
  await expect(dialog).toBeVisible();
  // The card opens whole: its comments and attachments are read too.
  await expect(dialog.getByTestId('card-comments')).toBeVisible();
  await expect(page.locator('[data-problem]')).toHaveCount(0);

  // The body is not the server's yet: writing one says so, as that.
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  await page.getByTestId('card-body').locator('[contenteditable]').first().click();
  await page.keyboard.type('Bring a list');
  await expect(page.locator('[data-problem]')).toHaveText(/not saved to the server yet/);
  await page.locator('[data-problem]').getByRole('button').click();
  await page.goBack();
  await expect(dialog).toBeHidden();
  await expect(column(page, 'Doing').locator('[data-card]')).toHaveCount(1);

  // On the server, in that column.
  const cards = (await (
    await api.get(`/api/v1/boards/${board.boardId}/cards`, { params: { perPage: 0 } })
  ).json()) as Schemas['Card'][];
  expect(cards).toHaveLength(1);
  expect(cards[0]?.properties).toEqual({ status: board.options.Doing });

  await page.reload();
  await expect(column(page, 'Doing').locator('[data-card]')).toHaveCount(1);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  await api.dispose();
  expect(errors).toEqual([]);
});
