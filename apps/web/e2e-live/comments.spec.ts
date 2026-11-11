/**
 * Comments on the real server: written, changed and deleted from the card,
 * their text a body document; seen by another person as they happen; and a
 * change made on a stale version not stored over somebody else's.
 */
import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

import {
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Person,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US' });

interface Node {
  type: string;
  text?: string;
  content?: Node[];
}

/** The text of a body, a line per block. */
function lines(body: unknown): string[] {
  const words = (node: Node): string =>
    node.text ?? (node.content ?? []).map((child) => words(child)).join('');
  return ((body as Node | null)?.content ?? []).map((block) => words(block));
}

async function commentsOn(
  api: APIRequestContext,
  boardId: string,
  cardId: string,
): Promise<Schemas['Block'][]> {
  return (await (
    await api.get(`/api/v1/boards/${boardId}/blocks`, {
      params: { parentId: cardId, type: 'comment' },
    })
  ).json()) as Schemas['Block'][];
}

async function openCard(page: Page, path: string, who: Person): Promise<void> {
  await page.goto(path);
  await signInOnPage(page, who);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('card-comments')).toBeVisible();
}

async function send(page: Page, text: string): Promise<void> {
  await page.getByTestId('comment-field').fill(text);
  await page.getByTestId('comment-send').click();
}

test('a comment is written, changed and deleted on the server, its text a document', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('talker');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Talk',
    cards: [{ title: 'Choose a date', status: 'To do' }],
  });
  const cardId = board.cards['Choose a date'] ?? '';
  const stored = () => commentsOn(api, board.boardId, cardId);

  const errors = watchErrors(page);
  await openCard(page, `/board/${board.boardId}/${board.viewId}/${cardId}`, owner);
  await send(page, 'How about Friday?\nOr Monday');
  const comments = page.getByTestId('card-comments');
  await expect(comments.locator('[data-comment]')).toHaveCount(1);
  await expect
    .poll(async () => (await stored()).map((one) => lines(one.body)))
    .toEqual([['How about Friday?', 'Or Monday']]);

  await comments.getByRole('button', { name: /^Edit the comment from/ }).click();
  const field = comments.getByTestId('comment-edit').locator('textarea');
  await expect(field).toHaveValue('How about Friday?\nOr Monday');
  await field.fill('Friday it is');
  await comments.getByRole('button', { name: 'Save' }).click();
  await expect(comments.getByTestId('comment-edit')).toHaveCount(0);
  await expect(comments.locator('[data-comment]')).toHaveText(/Friday it is/);
  await expect
    .poll(async () => (await stored()).map((one) => lines(one.body)))
    .toEqual([['Friday it is']]);

  await page.reload();
  await expect(comments.locator('[data-comment]')).toHaveText(/Friday it is/);
  await comments.getByRole('button', { name: /^Delete the comment from/ }).click();
  await expect(comments.locator('[data-comment]')).toHaveCount(0);
  await expect.poll(stored).toEqual([]);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  await api.dispose();
  expect(errors).toEqual([]);
});

test('a commenter’s comment shows on the owner’s open card, and only its writer may change it', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('host');
  const guest = person('guest');
  await register(base, owner);
  await register(base, guest);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Guests',
    cards: [{ title: 'Menu', status: 'To do' }],
    members: [{ who: guest, role: 'commenter' }],
  });
  const cardId = board.cards.Menu ?? '';
  const path = `/board/${board.boardId}/${board.viewId}/${cardId}`;

  const hostPage = await (await browser.newContext({ locale: 'en-US' })).newPage();
  const guestPage = await (await browser.newContext({ locale: 'en-US' })).newPage();
  const errors = [...watchErrors(hostPage), ...watchErrors(guestPage)];
  await openCard(hostPage, path, owner);
  await openCard(guestPage, path, guest);

  await send(guestPage, 'No mushrooms, please');
  const onHost = hostPage.getByTestId('card-comments').locator('[data-comment]');
  await expect(onHost).toHaveText(/No mushrooms, please/);
  // The owner does not change somebody else's words; the commenter cannot change any.
  await expect(hostPage.getByRole('button', { name: /^Edit the comment from/ })).toHaveCount(0);
  await expect(guestPage.getByRole('button', { name: /^Edit the comment from/ })).toHaveCount(0);
  expect((await commentsOn(api, board.boardId, cardId)).map((one) => lines(one.body))).toEqual([
    ['No mushrooms, please'],
  ]);

  await hostPage.context().close();
  await guestPage.context().close();
  await api.dispose();
  expect(errors).toEqual([]);
});

test('a comment changed on a stale version is refused, shows theirs, and keeps the text to save again', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('reviser');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Revise',
    cards: [{ title: 'Draft the letter', status: 'To do' }],
  });
  const cardId = board.cards['Draft the letter'] ?? '';
  const stored = () => commentsOn(api, board.boardId, cardId);
  const paragraph = (text: string) => ({
    type: 'doc',
    content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
  });

  const errors = watchErrors(page);
  await openCard(page, `/board/${board.boardId}/${board.viewId}/${cardId}`, owner);
  await send(page, 'First words');
  const comments = page.getByTestId('card-comments');
  await expect(comments.locator('[data-comment]')).toHaveText(/First words/);
  const [comment] = await stored();
  const commentId = comment?.id ?? '';

  // The page's change waits until the comment was changed elsewhere (as the same person, in another tab).
  let first = true;
  await page.route(`**/api/v1/boards/${board.boardId}/blocks/${commentId}`, async (route) => {
    if (route.request().method() === 'PATCH' && first) {
      first = false;
      const [now] = await stored();
      const changed = await api.patch(`/api/v1/boards/${board.boardId}/blocks/${commentId}`, {
        data: { body: paragraph('Words from the other tab'), readAt: now?.updateAt },
      });
      expect(changed.status()).toBe(204);
    }
    await route.continue();
  });

  await comments.getByRole('button', { name: /^Edit the comment from/ }).click();
  const field = comments.getByTestId('comment-edit').locator('textarea');
  await field.fill('Better words');
  await comments.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('[data-problem]')).toBeVisible();
  // Not stored over theirs; the field keeps what was written.
  expect((await stored()).map((one) => lines(one.body))).toEqual([['Words from the other tab']]);
  await expect(field).toHaveValue('Better words');

  await comments.getByRole('button', { name: 'Save' }).click();
  await expect(comments.getByTestId('comment-edit')).toHaveCount(0);
  await expect
    .poll(async () => (await stored()).map((one) => lines(one.body)))
    .toEqual([['Better words']]);
  await api.dispose();
  expect(errors).toEqual([]);
});
