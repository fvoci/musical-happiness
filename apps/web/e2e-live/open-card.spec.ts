/**
 * A card open while somebody else changes what the reader may do with it, on
 * the real server: taken off the board, the card closes and the page says
 * the board is not theirs to see; made a viewer, the body turns read-only and
 * says why; the card deleted, it says the card is gone, and once restored it
 * holds what was typed last. All while the admin is in the same card in
 * another browser.
 */
import path from 'node:path';

import { expect, test, type Browser, type Page } from '@playwright/test';

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
  type Person,
} from './support.ts';

test.use({ locale: 'en-US' });

/** How soon the reader's page follows the change: the server tells the room at once. */
const PROMPTLY = { timeout: 5_000 };

function editor(page: Page) {
  return page.getByTestId('card-body').locator('[contenteditable]').first();
}

async function cardOpen(browser: Browser, address: string, who: Person): Promise<Page> {
  const page = await (await browser.newContext({ locale: 'en-US' })).newPage();
  await page.goto(address);
  await signInOnPage(page, who);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  return page;
}

/** The pictures of the screen, as wide as the page is and 390 pixels wide, with `FVOCI_SCREENS=1`. */
async function shot(page: Page, name: string): Promise<void> {
  if (process.env.FVOCI_SCREENS !== '1') return;
  const folder = screensDir('collab-r4');
  const wide = page.viewportSize() ?? { width: 1280, height: 720 };
  await page.screenshot({ path: path.join(folder, `${name}-wide.png`) });
  await page.setViewportSize({ width: 390, height: 844 });
  // The frame of the card is laid out again for the narrow page before the picture.
  await page.evaluate(
    () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))),
  );
  await page.screenshot({ path: path.join(folder, `${name}-390.png`) });
  await page.setViewportSize(wide);
}

/** The admin and a colleague with the card open in two browsers, the colleague an editor. */
async function bothInTheCard(browser: Browser, base: string, title: string) {
  const admin = person('admin');
  const colleague = person('colleague');
  await register(base, admin);
  await register(base, colleague);
  const api = await signedIn(base, admin);
  const board = await makeBoard(api, {
    title,
    cards: [{ title: 'Agenda', status: 'To do' }],
    members: [{ who: colleague, role: 'editor' }],
  });
  const cardId = board.cards.Agenda ?? '';
  const address = boardPath(board, board.viewId, cardId);
  const first = await cardOpen(browser, address, admin);
  const second = await cardOpen(browser, address, colleague);
  const found = (await (
    await api.get(`/api/v1/w/${board.workspaceId}/users`, {
      params: { search: colleague.username },
    })
  ).json()) as Schemas['User'][];
  const colleagueId = found.find((user) => user.username === colleague.username)?.id ?? '';
  const memberPath = `/api/v1/w/${board.workspaceId}/boards/${board.boardId}/members/${colleagueId}`;
  return { api, first, second, memberPath, board, cardId };
}

test('taken off the board while the card is open: the card closes, and the page says why', async ({
  browser,
  baseURL,
}) => {
  const { api, first, second, memberPath } = await bothInTheCard(
    browser,
    baseURL ?? '',
    'Taken off',
  );
  // Read at the end: errors that come later are in them too.
  const watched = [watchErrors(first), watchErrors(second)];
  await editor(second).click();
  await second.keyboard.type('Bora was here');
  await expect(editor(first)).toContainText('Bora was here');

  expect((await api.delete(memberPath)).ok()).toBe(true);
  await expect(second.getByTestId('card-dialog')).toHaveCount(0, PROMPTLY);
  await expect(second.getByTestId('not-found')).toBeVisible();
  await shot(second, 'taken-off');

  // The admin goes on in the card, alone.
  await editor(first).click();
  await first.keyboard.press('End');
  await first.keyboard.type(' and left');
  await expect(editor(first)).toContainText('Bora was here and left');
  await expect(first.getByTestId('body-room-note')).toHaveCount(0);
  await first.context().close();
  await second.context().close();
  await api.dispose();
  expect(watched.flat()).toEqual([]);
});

test('made a viewer while the card is open: the body turns read-only, and says why', async ({
  browser,
  baseURL,
}) => {
  const { api, first, second, memberPath } = await bothInTheCard(
    browser,
    baseURL ?? '',
    'Made a viewer',
  );
  const watched = [watchErrors(first), watchErrors(second)];
  const role = await api.put(memberPath, {
    data: {
      schemeViewer: true,
      schemeCommenter: false,
      schemeEditor: false,
      schemeAdmin: false,
    },
  });
  expect(role.ok()).toBe(true);
  await expect(second.getByTestId('card-body')).toHaveAttribute('data-editor', 'reading', PROMPTLY);
  await expect(second.getByTestId('body-room-note')).toHaveText(
    'You can read this description but not change it.',
  );
  await expect(second.getByTestId('card-body').locator('[contenteditable="true"]')).toHaveCount(0);
  await shot(second, 'made-viewer');

  // Still in the room: what the admin types shows, and nothing typed here goes anywhere.
  await editor(first).click();
  await first.keyboard.type('Decided: Monday');
  await expect(editor(second)).toHaveText('Decided: Monday');
  await editor(second).click({ force: true });
  await second.keyboard.type('nope');
  await expect(editor(second)).toHaveText('Decided: Monday');
  await expect(editor(first)).toHaveText('Decided: Monday');
  await first.context().close();
  await second.context().close();
  await api.dispose();
  expect(watched.flat()).toEqual([]);
});

test('deleted while open: the card says it is gone, and restored it holds what was typed last', async ({
  browser,
  baseURL,
}) => {
  const { api, first, second, board, cardId } = await bothInTheCard(
    browser,
    baseURL ?? '',
    'Deleted meanwhile',
  );
  const watched = [watchErrors(first), watchErrors(second)];
  await editor(second).click();
  await second.keyboard.type('the last words');
  await expect(editor(first)).toContainText('the last words');

  // At once, before the room has written the body to the card.
  const card = `/api/v1/w/${board.workspaceId}/boards/${board.boardId}/blocks/${cardId}`;
  expect((await api.delete(card)).ok()).toBe(true);
  await expect(second.getByTestId('card-missing')).toBeVisible(PROMPTLY);
  await shot(second, 'deleted');
  await second.getByTestId('card-missing').getByRole('button', { name: 'Close' }).click();
  await expect(second.getByTestId('card-dialog')).toHaveCount(0);

  expect((await api.post(`${card}/undelete`)).ok()).toBe(true);
  const restored = (await (
    await api.get(`/api/v1/w/${board.workspaceId}/cards/${cardId}`)
  ).json()) as Schemas['Card'];
  expect(JSON.stringify(restored.body)).toContain('the last words');
  await first.context().close();
  await second.context().close();
  await api.dispose();
  expect(watched.flat()).toEqual([]);
});
