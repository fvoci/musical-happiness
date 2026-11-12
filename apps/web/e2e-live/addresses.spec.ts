/**
 * The addresses on the real server: every one starts at a workspace
 * (`/w/<workspace>/boards/<board>/<view>/<card>`), the root takes the reader
 * to one of theirs, and an address that does not fit is not found, the same
 * whatever does not fit.
 */
import { expect, test, type Page } from '@playwright/test';

import { boardLink, sidebar } from '../e2e/shell.support.ts';

import {
  addWorkspace,
  boardPath,
  makeBoard,
  person,
  placeOf,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Person,
} from './support.ts';

test.use({ locale: 'en-US' });

const NOT_THERE = '01930000-0000-7000-8000-00000000ffff';

async function signOut(page: Page): Promise<void> {
  await page.getByTestId('open-settings').click();
  await page.getByTestId('settings-dialog').getByRole('button', { name: 'Sign out' }).click();
  await expect(page.getByTestId('sign-in-form')).toBeVisible();
}

async function readerWithBoard(base: string): Promise<{
  who: Person;
  board: Awaited<ReturnType<typeof makeBoard>>;
}> {
  const who = person('addresses');
  await register(base, who);
  const api = await signedIn(base, who);
  const board = await makeBoard(api, {
    title: 'Home board',
    cards: [{ title: 'Paint the fence', status: 'To do' }],
  });
  await api.dispose();
  return { who, board };
}

test('signing in lands in a workspace of the reader; a card’s address opens it again after a reload', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const { who, board } = await readerWithBoard(base);
  const errors = watchErrors(page);

  await page.goto('/');
  await signInOnPage(page, who);
  // The workspace of the reader (shared with the others of this server), on its first board.
  await expect(page).toHaveURL(
    new RegExp(`^${base}/w/${board.workspaceId}/boards/[0-9a-f-]{36}/[0-9a-f-]{36}$`),
  );
  await expect(page.getByTestId('board-page')).toBeVisible();

  await boardLink(page, 'Home board').click();
  await expect(page).toHaveURL(new RegExp(`${boardPath(board, board.viewId)}$`));
  await page.getByTestId('board-page').getByText('Paint the fence').click();
  const cardId = board.cards['Paint the fence'] ?? '';
  await expect(page).toHaveURL(new RegExp(`${boardPath(board, board.viewId, cardId)}$`));
  const dialog = page.getByTestId('card-dialog');
  await expect(dialog).toBeVisible();

  await page.reload();
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId('card-title')).toHaveValue('Paint the fence');
  expect(placeOf(page.url())).toEqual({
    workspaceId: board.workspaceId,
    boardId: board.boardId,
    viewId: board.viewId,
    cardId,
  });
  expect(errors).toEqual([]);
});

test('an address that does not fit is not found, the same whatever does not fit', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const { who, board } = await readerWithBoard(base);
  // A second workspace of the reader, with a board of its own.
  const second = addWorkspace(base, who, 'Second');
  const api = await signedIn(base, who);
  const there = await makeBoard(api, { title: 'Away board', workspaceId: second });
  await api.dispose();
  // A workspace of somebody else.
  const stranger = person('stranger');
  await register(base, stranger);
  const theirs = addWorkspace(base, stranger, 'Theirs');

  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId));
  await signInOnPage(page, who);
  await expect(page.getByTestId('board-page')).toBeVisible();

  const notFound = page.getByTestId('not-found');
  const seen: string[] = [];
  for (const address of [
    // No such workspace, with a board the reader has.
    boardPath({ workspaceId: NOT_THERE, boardId: board.boardId }, board.viewId),
    // A workspace that is not the reader's.
    `/w/${theirs}`,
    // A board of the reader's other workspace, at this one's address.
    boardPath({ workspaceId: board.workspaceId, boardId: there.boardId }),
    // No such board.
    boardPath({ workspaceId: board.workspaceId, boardId: NOT_THERE }),
  ]) {
    await page.goto(address);
    await expect(notFound).toBeVisible();
    await expect(page.getByTestId('board-page')).toHaveCount(0);
    await expect(page.getByText('Away board')).toHaveCount(0);
    seen.push(await notFound.innerText());
  }
  expect(new Set(seen).size).toBe(1);

  // At its own workspace's address it is there.
  await page.goto(boardPath(there));
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`${boardPath(there, there.viewId)}$`));
  expect(errors).toEqual([]);
});

test('a board found in another workspace opens there, and nothing of the workspace left stays', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const { who, board } = await readerWithBoard(base);
  const second = addWorkspace(base, who, 'Second');
  const api = await signedIn(base, who);
  const word = `plum${Date.now().toString(36)}`;
  const there = await makeBoard(api, { title: `Away ${word}`, workspaceId: second });
  await api.dispose();

  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId));
  await signInOnPage(page, who);
  await expect(boardLink(page, 'Home board')).toBeVisible();
  await page.getByTestId('board-page').getByText('Paint the fence').click();
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await page.goBack();
  await expect(page.getByTestId('card-dialog')).toBeHidden();

  // The search looks in every workspace; the board found is linked at its own.
  await sidebar(page).getByTestId('sidebar-search').fill(word);
  const found = sidebar(page)
    .getByTestId('search-more')
    .getByRole('link', { name: `Away ${word}` });
  await expect(found).toHaveAttribute('href', boardPath(there));
  await found.click();
  await expect(page).toHaveURL(new RegExp(`${boardPath(there, there.viewId)}$`));
  await sidebar(page).getByTestId('sidebar-search').fill('');
  await expect(boardLink(page, `Away ${word}`)).toBeVisible();
  await expect(boardLink(page, 'Home board')).toHaveCount(0);
  await expect(page.getByText('Paint the fence')).toHaveCount(0);

  // Back: the first workspace again, as it was.
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`${boardPath(board, board.viewId)}$`));
  await expect(boardLink(page, 'Home board')).toBeVisible();
  await expect(boardLink(page, `Away ${word}`)).toHaveCount(0);
  await expect(page.getByText('Paint the fence')).toBeVisible();

  // Forward, and then the root: the workspace last entered on this browser.
  await page.goForward();
  await expect(boardLink(page, `Away ${word}`)).toBeVisible();
  await page.goto('/');
  await expect(page).toHaveURL(new RegExp(`${boardPath(there, there.viewId)}$`));
  expect(errors).toEqual([]);
});

test('the workspace kept on this browser is each person’s own', async ({ page, baseURL }) => {
  const base = baseURL ?? '';
  const { who, board } = await readerWithBoard(base);
  const second = addWorkspace(base, who, 'Second');
  const api = await signedIn(base, who);
  const there = await makeBoard(api, { title: 'Away board', workspaceId: second });
  await api.dispose();
  const other = await readerWithBoard(base);

  await page.goto(boardPath(there));
  await signInOnPage(page, who);
  await expect(page.getByTestId('board-page')).toBeVisible();
  await signOut(page);

  // Somebody else signs in here: their own workspace, not the one kept for the first.
  await signInOnPage(page, other.who);
  await expect(page).toHaveURL(new RegExp(`^${base}/w/${other.board.workspaceId}/boards/`));
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(boardLink(page, 'Away board')).toHaveCount(0);
  await signOut(page);

  // The first again, at the root: where they were last.
  await signInOnPage(page, who);
  await expect(page).toHaveURL(new RegExp(`${boardPath(there, there.viewId)}$`));
  expect(board.workspaceId).not.toBe(second);
});
