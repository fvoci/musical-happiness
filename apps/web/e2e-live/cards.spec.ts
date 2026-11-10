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

test('a card’s title and values are changed on the server, the last value taken away too', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('editor');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'House',
    cards: [
      { title: 'Paint the shed', status: 'To do' },
      { title: 'Fix the gate', status: 'To do', note: 'the latch' },
    ],
  });
  const cardId = board.cards['Paint the shed'] ?? '';
  const stored = async (): Promise<Schemas['Card']> =>
    (await (await api.get(`/api/v1/cards/${cardId}`)).json()) as Schemas['Card'];

  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${board.viewId}`);
  await signInOnPage(page, owner);
  await column(page, 'To do').getByText('Paint the shed').click();
  const dialog = page.getByTestId('card-dialog');
  await expect(dialog).toBeVisible();

  await page.getByTestId('card-title').fill('Paint the whole shed');
  await expect.poll(async () => (await stored()).title).toBe('Paint the whole shed');

  const status = dialog
    .locator('[data-property]')
    .filter({ has: page.locator('dt', { hasText: /^\s*Status\s*$/ }) });
  await status.locator('[data-slot="property-select"]').click();
  await page.getByRole('option', { name: 'Done' }).click();
  await expect
    .poll(async () => (await stored()).properties)
    .toEqual({ status: board.options.Done });

  // The last value of the card: the server's card patch cannot take it away.
  await status.locator('[data-slot="property-select"]').click();
  await page.getByRole('button', { name: 'Clear' }).click();
  await expect.poll(async () => (await stored()).properties).toEqual({});
  expect((await stored()).title).toBe('Paint the whole shed');

  await page.goBack();
  await expect(dialog).toBeHidden();

  // A card with another value: changing one keeps the other.
  const gate = board.cards['Fix the gate'] ?? '';
  await column(page, 'To do').getByText('Fix the gate').click();
  await status.locator('[data-slot="property-select"]').click();
  await page.getByRole('option', { name: 'Doing' }).click();
  await expect
    .poll(
      async () =>
        ((await (await api.get(`/api/v1/cards/${gate}`)).json()) as Schemas['Card']).properties,
    )
    .toEqual({ status: board.options.Doing, note: 'the latch' });
  await page.goBack();

  await page.reload();
  await expect(page.getByText('Paint the whole shed')).toBeVisible();
  await expect(column(page, 'To do').locator('[data-card]')).toHaveCount(0);
  await expect(column(page, 'Done').locator('[data-card]')).toHaveCount(0);
  await expect(column(page, 'Doing').getByText('Fix the gate')).toBeVisible();
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  await api.dispose();
  expect(errors).toEqual([]);
});
