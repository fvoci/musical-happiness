/**
 * Notifications on the real server, two people each in a browser of their
 * own: a comment one writes on a card the other wrote rings the other's bell
 * without a reload; the notification opens the card at the comment and is
 * read by it; and the switch of comments, turned off in the settings, is kept
 * by the server, which tells nothing of comments while it is off.
 */
import { expect, test, type APIRequestContext, type Locator, type Page } from '@playwright/test';

import { showSidebar, sidebar, strip } from '../e2e/shell.support.ts';
import {
  boardPath,
  makeBoard,
  type MadeBoard,
  person,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

const bell = (page: Page): Locator => page.getByTestId('open-notifications');
const badge = (page: Page): Locator => page.getByTestId('unread-notifications');
const inbox = (page: Page): Locator => page.getByTestId('notifications');

async function send(page: Page, text: string): Promise<void> {
  await page.getByTestId('comment-field').fill(text);
  await page.getByTestId('comment-send').click();
}

/** The switch of comments in the settings, opened from the reader's account. */
async function commentsSwitch(page: Page): Promise<Locator> {
  if (await strip(page).isVisible()) {
    await strip(page).getByTestId('strip-open-settings').click();
  } else {
    await showSidebar(page);
    await sidebar(page).getByTestId('open-settings').click();
  }
  const dialog = page.getByTestId('settings-dialog');
  await expect(dialog).toBeVisible();
  return dialog.locator('[data-setting="notifyComments"]').getByRole('switch');
}

async function closeSettings(page: Page): Promise<void> {
  await page.getByTestId('close-settings').click();
  await expect(page.getByTestId('settings-dialog')).toBeHidden();
}

async function settings(api: APIRequestContext): Promise<Record<string, unknown>> {
  return (await (await api.get('/api/v1/users/me/settings')).json()) as Record<string, unknown>;
}

/**
 * A writer and a commenter of a board, each signed in to the API: the writer
 * made its two cards, and who writes a card watches it.
 */
async function aWatchedBoard(base: string) {
  const writer = person('watcher');
  const talker = person('talker');
  await register(base, writer);
  await register(base, talker);
  const api = await signedIn(base, writer);
  const board = await makeBoard(api, {
    title: 'Told',
    cards: [
      { title: 'Plan the week', status: 'To do' },
      { title: 'Book the room', status: 'To do' },
    ],
    members: [{ who: talker, role: 'commenter' }],
  });
  const me = (await (await api.get('/api/v1/users/me')).json()) as Schemas['User'];
  return { writer, talker, api, board, me };
}

type Inline = { type: 'text'; text: string } | { type: 'mention'; attrs: { userId: string } };

/** Writes a comment on a card through the API, as whoever `api` is signed in as. */
async function comment(
  api: APIRequestContext,
  board: MadeBoard,
  cardId: string,
  content: Inline[],
): Promise<void> {
  const written = await api.post(`/api/v1/w/${board.workspaceId}/boards/${board.boardId}/blocks`, {
    data: [
      {
        containerId: board.boardId,
        parentId: cardId,
        type: 'comment',
        title: '',
        createAt: 1,
        updateAt: 1,
        fields: {},
        body: { type: 'doc', content: [{ type: 'paragraph', content }] },
      },
    ],
  });
  expect(written.ok(), await written.text()).toBe(true);
}

test('a comment on a card one wrote rings their bell at once, and opens the card at it', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const { writer, talker, api, board } = await aWatchedBoard(base);
  const planned = board.cards['Plan the week'] ?? '';

  const theirs = await (await browser.newContext({ locale: 'en-US', baseURL: base })).newPage();
  const mine = await (await browser.newContext({ locale: 'en-US', baseURL: base })).newPage();
  const errors = [theirs, mine].map(watchErrors);
  await mine.goto(boardPath(board, board.viewId));
  await signInOnPage(mine, writer);
  await expect(bell(mine)).toBeVisible();
  await expect(badge(mine)).toBeHidden();
  await theirs.goto(boardPath(board, board.viewId, planned));
  await signInOnPage(theirs, talker);
  await expect(theirs.getByTestId('card-comments')).toBeVisible();

  // The other person comments: the writer's bell rings, no reload.
  await send(theirs, 'How about Friday?');
  const said = theirs.getByTestId('card-comments').locator('[data-comment]');
  await expect(said).toHaveCount(1);
  const commentId = (await said.first().getAttribute('data-comment')) ?? '';
  await expect(badge(mine)).toHaveText('1');

  // Opened from the bell: the card, at the comment, and read.
  await bell(mine).click();
  const line = inbox(mine).getByRole('link', { name: /commented on Plan the week/ });
  await expect(line).toBeVisible();
  await line.click();
  await expect(mine.getByTestId('card-dialog')).toBeVisible();
  await expect(mine.getByTestId('card-title')).toHaveValue('Plan the week');
  await expect(mine.locator(`[data-comment="${commentId}"]`)).toBeInViewport();
  await expect(badge(mine)).toBeHidden();
  // Read on the server too.
  const unread = async (): Promise<unknown> =>
    ((await (await api.get('/api/v1/notifications/unread')).json()) as { count: number }).count;
  await expect.poll(unread).toBe(0);

  await api.dispose();
  expect(errors.flat()).toEqual([]);
});

test('the switch of comments is kept by the server, which tells nothing of comments while it is off', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const { writer, talker, api, board, me } = await aWatchedBoard(base);
  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId));
  await signInOnPage(page, writer);

  const off = await commentsSwitch(page);
  await expect(off).toBeChecked();
  await off.click();
  await expect(off).not.toBeChecked();
  await expect.poll(() => settings(api)).toMatchObject({ notifyComments: false });
  await closeSettings(page);
  await page.reload();
  await expect(bell(page)).toBeVisible();
  await expect(await commentsSwitch(page)).not.toBeChecked();
  await closeSettings(page);

  // Off, a comment tells nothing. A mention after it, on the other card, is
  // told: by the time it rings, the comment before it was worked off.
  const talkerApi = await signedIn(base, talker);
  await comment(talkerApi, board, board.cards['Plan the week'] ?? '', [
    { type: 'text', text: 'How about Friday?' },
  ]);
  await comment(talkerApi, board, board.cards['Book the room'] ?? '', [
    { type: 'text', text: 'Can you book it, ' },
    { type: 'mention', attrs: { userId: me.id } },
  ]);
  await expect(badge(page)).toHaveText('1');
  await bell(page).click();
  await expect(
    inbox(page).getByRole('link', { name: /mentioned you in Book the room/ }),
  ).toBeVisible();
  await expect(inbox(page).locator('[data-notification]')).toHaveCount(1);

  await talkerApi.dispose();
  await api.dispose();
  expect(errors).toEqual([]);
});
