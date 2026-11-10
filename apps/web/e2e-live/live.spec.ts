/**
 * Changes as they happen on the real server: two people on one board, each
 * in a browser of their own. What one does shows on the other's page
 * without a reload; and a page whose connection dropped reads the board
 * again once it is back, since the server does not send what it missed.
 */
import { expect, test, type Page, type WebSocketRoute } from '@playwright/test';

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

test('what one person does shows on the other’s page, and a dropped connection catches up', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('first');
  const other = person('second');
  await register(base, owner);
  await register(base, other);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Shared',
    cards: [{ title: 'Plan the week', status: 'To do' }],
    members: [{ who: other, role: 'editor' }],
  });
  const address = `/board/${board.boardId}/${board.viewId}`;

  const first = await (await browser.newContext({ locale: 'en-US', baseURL: base })).newPage();
  const second = await (await browser.newContext({ locale: 'en-US', baseURL: base })).newPage();
  const errors = [...[first, second].map(watchErrors)];

  // The second page's connection goes through the test, which can drop it.
  let open: WebSocketRoute | undefined;
  let refusing = false;
  await second.routeWebSocket(/\/ws$/, (socket) => {
    if (refusing) {
      void socket.close();
      return;
    }
    socket.connectToServer();
    open = socket;
  });

  for (const [page, who] of [
    [first, owner],
    [second, other],
  ] as const) {
    await page.goto(address);
    await signInOnPage(page, who);
    await expect(column(page, 'To do').getByText('Plan the week')).toBeVisible();
  }

  // A title changed on the first page shows on the second.
  await column(first, 'To do').getByText('Plan the week').click();
  await first.getByTestId('card-title').fill('Plan the month');
  await expect(column(second, 'To do').getByText('Plan the month')).toBeVisible();
  await first.goBack();

  // A card made on the second page shows on the first.
  await second.getByRole('button', { name: 'New card in Done' }).click();
  await second.getByTestId('card-title').fill('Water the plants');
  await second.goBack();
  await expect(column(first, 'Done').getByText('Water the plants')).toBeVisible();

  // The second page's connection drops; while it is away, a card is made.
  await expect.poll(() => open !== undefined).toBe(true);
  refusing = true;
  await open?.close();
  const made = (await (
    await api.post(`/api/v1/boards/${board.boardId}/cards`, {
      data: { title: 'Missed while away', properties: { status: board.options.Doing } },
    })
  ).json()) as Schemas['Card'];
  expect(made.title).toBe('Missed while away');
  await expect(column(first, 'Doing').getByText('Missed while away')).toBeVisible();
  await expect(column(second, 'Doing').getByText('Missed while away')).toHaveCount(0);

  // Back: the board is read again, and the card that was missed is there.
  refusing = false;
  await expect(column(second, 'Doing').getByText('Missed while away')).toBeVisible({
    timeout: 20_000,
  });
  await expect(second.locator('[data-problem]')).toHaveCount(0);

  await api.dispose();
  await first.context().close();
  await second.context().close();
  expect(errors.flat()).toEqual([]);
});
