/**
 * A card's body on the real server: written from the editor, read back the
 * same, and written again on a newer version when the card changed in
 * between (409), either by itself when only something else changed or by
 * asking the reader when somebody else wrote the body.
 */
import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

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

interface Node {
  type: string;
  text?: string;
  content?: Node[];
}

/** The text of a body, a line per block of the document. */
function lines(body: unknown): string[] {
  const words = (node: Node): string =>
    node.text ?? (node.content ?? []).map((child) => words(child)).join('');
  return ((body as Node | null)?.content ?? []).map((block) => words(block));
}

async function stored(
  api: APIRequestContext,
  board: { workspaceId: string },
  cardId: string,
): Promise<Schemas['Card']> {
  return (await (
    await api.get(`/api/v1/w/${board.workspaceId}/cards/${cardId}`)
  ).json()) as Schemas['Card'];
}

function editor(page: Page) {
  return page.getByTestId('card-body').locator('[contenteditable]').first();
}

async function openCard(page: Page, path: string, who: Parameters<typeof signInOnPage>[1]) {
  await page.goto(path);
  await signInOnPage(page, who);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
}

test('a body written in the editor is the server’s, and opens again as it was', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('writer');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Notes',
    cards: [{ title: 'Write the report', status: 'To do' }],
  });
  const cardId = board.cards['Write the report'] ?? '';
  const writes: unknown[] = [];
  page.on('request', (request) => {
    if (request.method() === 'PATCH' && request.url().endsWith(`/cards/${cardId}`)) {
      writes.push(request.postDataJSON());
    }
  });

  const errors = watchErrors(page);
  await openCard(page, boardPath(board, board.viewId, cardId), owner);
  await editor(page).click();
  await page.keyboard.type('First line');
  await page.keyboard.press('Enter');
  await page.keyboard.type('Second line');
  await expect
    .poll(async () => lines((await stored(api, board, cardId)).body))
    .toEqual(['First line', 'Second line']);
  // Every write named the version it was read at.
  expect(writes.length).toBeGreaterThan(0);
  for (const write of writes) expect(write).toMatchObject({ readAt: expect.any(Number) });
  // The list of cards says there is one, without carrying it.
  const [listed] = (await (
    await api.get(`/api/v1/w/${board.workspaceId}/boards/${board.boardId}/cards`, {
      params: { perPage: 0 },
    })
  ).json()) as Schemas['Card'][];
  expect(listed).toMatchObject({ hasBody: true, body: null });

  const saved = (await stored(api, board, cardId)).body;
  // The board opened anew, the card from it.
  await page.goto(boardPath(board, board.viewId));
  await expect(page.getByTestId('card-dialog')).toBeHidden();
  const written = writes.length;
  await page.locator('[data-card]').filter({ hasText: 'Write the report' }).click();
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  await expect(editor(page).locator('p')).toHaveText(['First line', 'Second line']);
  // Opening it is no change: nothing is written, and the server keeps the same document.
  await editor(page).click();
  await page.waitForTimeout(1200);
  expect(writes).toHaveLength(written);
  expect((await stored(api, board, cardId)).body).toEqual(saved);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  await api.dispose();
  expect(errors).toEqual([]);
});

test('a body written while another value of the card changed is sent again on the new version', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('retrier');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Retry',
    cards: [{ title: 'Call the bank', status: 'To do' }],
  });
  const cardId = board.cards['Call the bank'] ?? '';
  const answers: number[] = [];
  page.on('response', (response) => {
    const request = response.request();
    if (request.method() === 'PATCH' && request.url().endsWith(`/cards/${cardId}`)) {
      answers.push(response.status());
    }
  });

  const errors = watchErrors(page);
  await openCard(page, boardPath(board, board.viewId, cardId), owner);
  // The first write of the body waits until somebody has changed a value of the card.
  let first = true;
  await page.route(`**/cards/${cardId}`, async (route) => {
    if (route.request().method() === 'PATCH' && first) {
      first = false;
      const changed = await api.patch(`/api/v1/w/${board.workspaceId}/cards/${cardId}`, {
        data: { updatedProperties: { note: 'ask for Kim' } },
      });
      expect(changed.ok()).toBe(true);
    }
    await route.continue();
  });
  await editor(page).click();
  await page.keyboard.type('Ask about the loan');

  await expect
    .poll(async () => lines((await stored(api, board, cardId)).body))
    .toEqual(['Ask about the loan']);
  // Refused once on the old version, then stored (a slow typist may make more writes after).
  await expect.poll(() => answers).toContain(200);
  expect(answers.slice(0, 2)).toEqual([409, 200]);
  expect((await stored(api, board, cardId)).properties).toMatchObject({ note: 'ask for Kim' });
  await expect(page.getByTestId('body-conflict')).toHaveCount(0);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  await api.dispose();
  expect(errors).toEqual([]);
});

test('a body somebody else wrote meanwhile is not overwritten until the reader chooses', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('chooser');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Conflict',
    cards: [{ title: 'Pack the boxes', status: 'To do' }],
  });
  const cardId = board.cards['Pack the boxes'] ?? '';
  const theirs = {
    type: 'doc',
    content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Their plan' }] }],
  };

  const errors = watchErrors(page);
  await openCard(page, boardPath(board, board.viewId, cardId), owner);
  // The first write of the body waits until somebody else has written one.
  let first = true;
  await page.route(`**/cards/${cardId}`, async (route) => {
    if (route.request().method() === 'PATCH' && first) {
      first = false;
      const readAt = (await stored(api, board, cardId)).updateAt;
      const written = await api.patch(`/api/v1/w/${board.workspaceId}/cards/${cardId}`, {
        data: { body: theirs, readAt },
      });
      expect(written.ok()).toBe(true);
    }
    await route.continue();
  });
  await editor(page).click();
  await page.keyboard.type('My plan');

  const notice = page.getByTestId('body-conflict');
  await expect(notice).toBeVisible();
  // Nothing was overwritten: the server has their text.
  expect(lines((await stored(api, board, cardId)).body)).toEqual(['Their plan']);
  await notice.getByTestId('conflict-mine').click();
  await expect(notice).toBeHidden();
  await expect
    .poll(async () => lines((await stored(api, board, cardId)).body))
    .toEqual(['My plan']);
  await expect(editor(page).locator('p')).toHaveText(['My plan']);
  await api.dispose();
  expect(errors).toEqual([]);
});
