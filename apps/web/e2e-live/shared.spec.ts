/**
 * A board read by its public link on the real server: a browser that is
 * signed in as nobody opens the link, reads the board, switches views, opens
 * a card and reads its body and comments, and sends nothing but readings.
 * Somebody signed in reads it the same way. A wrong token, and the link
 * turned off, show "not found". Pictures, wide and at 390 pixels, go to
 * `target/screens/shared/` when `FVOCI_SCREENS` is set.
 */
import { expect, test, type Page } from '@playwright/test';

import { screensDir } from '../e2e/screens.ts';

import {
  boardPath,
  makeBoard,
  person,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

/** The address of the board's public link, a view and a card on it. */
function sharedPath(boardId: string, token: string, viewId?: string, cardId?: string): string {
  const parts = [boardId, viewId, cardId].filter((part) => part !== undefined);
  return `/shared/${parts.map(encodeURIComponent).join('/')}?r=${encodeURIComponent(token)}`;
}

/**
 * What the page asks of the server that a reader by the link must not: a
 * request to the API that is no reading, or not of the board under
 * `/api/v1/shared`, or that carries a cookie; and any connection for changes.
 */
function strayRequestsOf(page: Page, boardId: string): string[] {
  const stray: string[] = [];
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (!url.pathname.startsWith('/api/')) return;
    const said = `${request.method()} ${url.pathname}`;
    if (request.method() !== 'GET' || !url.pathname.startsWith(`/api/v1/shared/${boardId}`)) {
      stray.push(said);
    }
    void request.allHeaders().then((headers) => {
      if (headers.cookie !== undefined) stray.push(`${said} with a cookie`);
    });
  });
  page.on('websocket', (socket) => stray.push(`websocket ${socket.url()}`));
  return stray;
}

test('a board is read by its public link without signing in, and not when the link is off', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('sharer');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Allotment plan',
    cards: [
      { title: 'Sow the beans', status: 'To do', note: 'after the frost' },
      { title: 'Mend the fence', status: 'Doing' },
      { title: 'Order compost', status: 'Done' },
    ],
  });
  const at = `/api/v1/w/${board.workspaceId}/boards/${board.boardId}`;
  const cardId = board.cards['Sow the beans'] ?? '';
  const card = (await (
    await api.get(`/api/v1/w/${board.workspaceId}/cards/${cardId}`)
  ).json()) as Schemas['Card'];
  const paragraph = (text: string) => ({ type: 'paragraph', content: [{ type: 'text', text }] });
  const body = { type: 'doc', content: [paragraph('Two rows, a hand apart.')] };
  expect(
    (
      await api.patch(`/api/v1/w/${board.workspaceId}/cards/${cardId}`, {
        data: { readAt: card.updateAt, body },
      })
    ).ok(),
  ).toBe(true);
  const commented = await api.post(`${at}/blocks`, {
    data: [
      {
        type: 'comment',
        parentId: cardId,
        boardId: board.boardId,
        title: '',
        body: { type: 'doc', content: [paragraph('The broad beans did well last year.')] },
        createAt: Date.now(),
        updateAt: Date.now(),
      },
    ],
  });
  expect(commented.ok()).toBe(true);
  const table = await api.post(`${at}/blocks`, {
    data: [
      {
        type: 'view',
        parentId: board.boardId,
        boardId: board.boardId,
        title: 'Table of it all',
        fields: { viewType: 'table', visiblePropertyIds: ['status', 'note'] },
        createAt: Date.now(),
        updateAt: Date.now(),
      },
    ],
  });
  expect(table.ok()).toBe(true);
  const shared = (await (
    await api.post(`${at}/sharing`, { data: { enabled: true } })
  ).json()) as Schemas['Sharing'];
  const token = shared.token ?? '';
  expect(token).not.toBe('');

  // Signed in as nobody.
  const context = await browser.newContext({ locale: 'en-US', baseURL: base });
  const page = await context.newPage();
  const errors = watchErrors(page);
  const writes = strayRequestsOf(page, board.boardId);
  await page.goto(sharedPath(board.boardId, token));
  const reader = page.getByTestId('shared-board');
  await expect(reader.getByTestId('board-page')).toBeVisible();
  await expect(reader.getByText('Sow the beans')).toBeVisible();
  await expect(reader.getByText('Mend the fence')).toBeVisible();
  await expect(reader.getByTestId('shared-title')).toHaveText('Allotment plan');
  // The board alone: no sidebar, no settings, no sharing, no way to add a card.
  await expect(page.getByTestId('sidebar')).toHaveCount(0);
  await expect(page.getByTestId('share-board')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /settings/i })).toHaveCount(0);
  await expect(page.getByRole('button', { name: /^new$/i })).toHaveCount(0);
  // The first view, in the address of the link.
  await expect(page).toHaveURL(new RegExp(`/shared/${board.boardId}/${board.viewId}\\?r=`));

  const shoot = async (name: string): Promise<void> => {
    if (process.env.FVOCI_SCREENS) {
      await page.screenshot({
        path: `${screensDir('shared')}/shared-${name}.png`,
        animations: 'disabled',
      });
    }
  };
  await page.setViewportSize({ width: 1280, height: 800 });
  await shoot('board-wide');

  // Another view, by its tab.
  await reader.getByRole('link', { name: 'Table of it all' }).first().click();
  await expect(page).toHaveURL(/\/shared\/[^/]+\/[^/?]+\?r=/);
  await expect(reader.getByText('after the frost')).toBeVisible();
  await shoot('table-wide');
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`/shared/${board.boardId}/${board.viewId}\\?r=`));

  // A card: its body and its comment, nobody named, no way to write.
  await reader.getByText('Sow the beans').click();
  const dialog = page.getByTestId('card-dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText('Two rows, a hand apart.')).toBeVisible();
  const comments = dialog.getByTestId('card-comments');
  await expect(comments.getByText('The broad beans did well last year.')).toBeVisible();
  await expect(comments.getByText(owner.username)).toHaveCount(0);
  await expect(dialog.getByRole('textbox')).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/${cardId}\\?r=`));
  await shoot('card-wide');

  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);

  // At 390 pixels, from the card's own address, as from the board's.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(sharedPath(board.boardId, token, board.viewId, cardId));
  await expect(dialog.getByText('Two rows, a hand apart.')).toBeVisible();
  await shoot('card-390');
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(reader.getByText('Sow the beans')).toBeVisible();
  await shoot('board-390');
  expect(writes).toEqual([]);

  // Somebody signed in, its admin here, reads it the same way, as a reader: no
  // cookie goes along, and nothing of the admin's shows.
  const ownerContext = await browser.newContext({ locale: 'en-US', baseURL: base });
  const ownerPage = await ownerContext.newPage();
  const ownerErrors = watchErrors(ownerPage);
  await ownerPage.goto(boardPath(board, board.viewId));
  await signInOnPage(ownerPage, owner);
  await expect(ownerPage.getByTestId('share-board')).toBeVisible();
  const ownerReader = await ownerContext.newPage();
  ownerErrors.push(...watchErrors(ownerReader));
  const ownerStray = strayRequestsOf(ownerReader, board.boardId);
  await ownerReader.goto(sharedPath(board.boardId, token));
  await expect(ownerReader.getByTestId('shared-board').getByText('Sow the beans')).toBeVisible();
  await expect(ownerReader.getByTestId('sidebar')).toHaveCount(0);
  await expect(ownerReader.getByTestId('share-board')).toHaveCount(0);
  await expect(ownerReader.getByRole('button', { name: /^new$/i })).toHaveCount(0);
  await ownerReader.getByTestId('shared-board').getByText('Sow the beans').click();
  const ownerDialog = ownerReader.getByTestId('card-dialog');
  await expect(ownerDialog.getByText('Two rows, a hand apart.')).toBeVisible();
  await expect(ownerDialog.getByRole('textbox')).toHaveCount(0);
  await expect(ownerDialog.getByTestId('card-comments').getByText(owner.username)).toHaveCount(0);
  expect(ownerStray).toEqual([]);

  // A wrong token: not found.
  await page.goto(sharedPath(board.boardId, `${token}x`));
  await expect(page.getByTestId('not-found')).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 800 });
  await shoot('not-found-wide');

  // The link turned off: not found, at the address that worked.
  expect((await api.post(`${at}/sharing`, { data: { enabled: false } })).ok()).toBe(true);
  await page.goto(sharedPath(board.boardId, token));
  await expect(page.getByTestId('not-found')).toBeVisible();
  await expect(page.getByText('Sow the beans')).toHaveCount(0);
  expect(writes).toEqual([]);
  // Its way home is the app's: the sign-in page, the page loaded anew.
  await page.getByRole('link', { name: 'Back to home' }).click();
  await expect(page.getByRole('button', { name: /^sign in$/i })).toBeVisible();

  await api.dispose();
  await context.close();
  await ownerContext.close();
  expect([...errors, ...ownerErrors]).toEqual([]);
});
