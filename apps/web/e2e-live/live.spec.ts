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
  // What the server sent it is kept, to tell later how a change arrived.
  let open: { page: WebSocketRoute; server: WebSocketRoute } | undefined;
  const heard: string[] = [];
  await second.routeWebSocket(/\/ws$/, (socket) => {
    const server = socket.connectToServer();
    server.onMessage((message) => {
      heard.push(String(message));
      socket.send(message);
    });
    open = { page: socket, server };
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

  // The second page's connection drops, both ways; while it is away, a card
  // is made. Its message reaches the first page only.
  await expect.poll(() => open !== undefined).toBe(true);
  const dropped = open;
  const readAgain = second.waitForRequest(
    (request) =>
      request.method() === 'GET' && request.url().includes(`/boards/${board.boardId}/cards`),
  );
  await dropped?.server.close();
  await dropped?.page.close();
  const made = (await (
    await api.post(`/api/v1/boards/${board.boardId}/cards`, {
      data: { title: 'Missed while away', properties: { status: board.options.Doing } },
    })
  ).json()) as Schemas['Card'];
  await expect(column(first, 'Doing').getByText('Missed while away')).toBeVisible();

  // Back: the board is read again, and the card that was missed is there,
  // brought by the reading: no message of the server named it.
  await readAgain;
  await expect(column(second, 'Doing').getByText('Missed while away')).toBeVisible();
  expect(heard.some((message) => message.includes('Missed while away'))).toBe(false);
  expect(heard.length).toBeGreaterThan(0);
  // And the new connection is a real one: a later change arrives by it.
  expect(open).not.toBe(dropped);
  await api.patch(`/api/v1/cards/${made.id}`, { data: { title: 'Caught up' } });
  await expect(column(second, 'Doing').getByText('Caught up')).toBeVisible();
  await expect(second.locator('[data-problem]')).toHaveCount(0);

  await api.dispose();
  await first.context().close();
  await second.context().close();
  expect(errors.flat()).toEqual([]);
});
