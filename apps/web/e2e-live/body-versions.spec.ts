/**
 * The versions of a card's body on the real server: two people in the
 * card's room, one of them goes back to a version and both have its text,
 * what was typed stays a version to go back to, and undo in the editor
 * does not take a restore back; a viewer reads the versions and restores
 * none; a reader by the public link is offered none.
 */
import { expect, test, type APIRequestContext, type Browser, type Page } from '@playwright/test';

import { focusAtEnd } from '../e2e/embeds.support.ts';
import { primaryKey } from '../e2e/keyboard.support.ts';

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

type Board = Awaited<ReturnType<typeof makeBoard>>;

const paragraph = (text: string): object => ({
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
});

const cardAt = (board: Board, cardId: string): string =>
  `/api/v1/w/${board.workspaceId}/cards/${cardId}`;

/** Writes a card's body whole through the API, as whoever `api` is signed in as. */
async function writeWhole(
  api: APIRequestContext,
  board: Board,
  cardId: string,
  text: string,
): Promise<void> {
  const card = (await (await api.get(cardAt(board, cardId))).json()) as Schemas['Card'];
  const written = await api.patch(cardAt(board, cardId), {
    data: { body: paragraph(text), readAt: card.updateAt },
  });
  expect(written.ok()).toBe(true);
}

async function kept(
  api: APIRequestContext,
  board: Board,
  cardId: string,
): Promise<Schemas['BodyVersion'][]> {
  const answer = await api.get(`${cardAt(board, cardId)}/body-versions`);
  expect(answer.ok()).toBe(true);
  return (await answer.json()) as Schemas['BodyVersion'][];
}

async function pageOf(
  browser: Browser,
  path: string,
  who: Parameters<typeof signInOnPage>[1],
): Promise<Page> {
  const page = await (await browser.newContext({ locale: 'en-US' })).newPage();
  await page.goto(path);
  await signInOnPage(page, who);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  return page;
}

const editor = (page: Page) => page.getByTestId('card-body').locator('[contenteditable]').first();
const versions = (page: Page) => page.getByTestId('body-versions');
const rows = (page: Page) => versions(page).locator('[data-version]');
const shown = (page: Page) => page.getByTestId('body-version');

/**
 * Types at the end of the body, once the room's document is all there: the
 * caret is put through the editor, which a click and a key can race while
 * the document is still being received. The editor takes the focus a frame
 * later, and a key before that goes nowhere.
 */
async function typeAtEnd(page: Page, text: string, newLine = false): Promise<void> {
  await focusAtEnd(page.getByTestId('card-body'));
  await expect(editor(page)).toBeFocused();
  if (newLine) await page.keyboard.press('Enter');
  await page.keyboard.type(text);
}

async function openVersions(page: Page): Promise<void> {
  await page.getByTestId('card-menu').click();
  await page.locator('[data-action="versions"]').click();
  await expect(versions(page)).toBeVisible();
}

/** Goes back to the version shown, after the question. */
async function restoreShown(page: Page): Promise<void> {
  await page.getByTestId('body-version-restore').click();
  await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Restore' }).click();
  await expect(versions(page)).toHaveCount(0);
}

test('a version restored is the body of everyone in the room, and what was typed stays a version', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('ada');
  const colleague = person('bora');
  await register(base, owner);
  await register(base, colleague);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Versions',
    cards: [{ title: 'Plan the launch', status: 'To do' }],
    members: [{ who: colleague, role: 'editor' }],
  });
  const cardId = board.cards['Plan the launch'] ?? '';
  // A card with a body: no body is no first version.
  await writeWhole(api, board, cardId, 'The first plan');
  const path = boardPath(board, board.viewId, cardId);
  const first = await pageOf(browser, path, owner);
  const second = await pageOf(browser, path, colleague);
  // Read at the end: errors that come later are in them too.
  const watched = [watchErrors(first), watchErrors(second)];
  for (const page of [first, second]) {
    await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
    await expect(editor(page).locator('p')).toHaveText(['The first plan']);
  }

  await typeAtEnd(second, 'Bora adds a line', true);
  await expect(editor(first).locator('p')).toHaveText(['The first plan', 'Bora adds a line']);
  // The owner writes too: each has something of their own to undo.
  await typeAtEnd(first, ', Ada another');
  const typed = ['The first plan', 'Bora adds a line, Ada another'];
  for (const page of [first, second]) await expect(editor(page).locator('p')).toHaveText(typed);

  // The owner goes back to the body from before.
  await openVersions(first);
  await expect(rows(first)).toHaveCount(1);
  await expect(rows(first).first().getByTestId('body-version-editors')).toContainText(
    owner.username,
  );
  await rows(first).first().click();
  await expect(shown(first)).toContainText('The first plan');
  await expect(shown(first)).not.toContainText('Bora adds a line');
  await expect(shown(first)).not.toContainText('Ada another');
  await restoreShown(first);
  for (const page of [first, second]) {
    await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
    await expect(editor(page).locator('p')).toHaveText(['The first plan']);
    // Nothing of theirs was waiting: nothing is said lost.
    await expect(page.getByTestId('body-lost')).toHaveCount(0);
  }
  const restored = (await (await api.get(cardAt(board, cardId))).json()) as Schemas['Card'];
  expect(restored.body).toEqual(paragraph('The first plan'));

  // The restore is no step of the editor's undo, for the one who restored or the other:
  // undoing brings back nothing either of them typed.
  for (const page of [first, second]) {
    await editor(page).click();
    await page.keyboard.press(await primaryKey(page, 'z'));
    await expect(editor(page).locator('p')).toHaveText(['The first plan']);
  }

  // What was typed is a version of its writer, and the body restored the newest, of who restored it.
  const list = await kept(api, board, cardId);
  expect(list.map((one) => [one.version, one.isCurrent])).toEqual([
    [3, true],
    [2, false],
    [1, false],
  ]);
  await openVersions(second);
  await expect(rows(second)).toHaveCount(3);
  await expect(rows(second).first().getByTestId('body-version-current')).toBeVisible();
  await expect(rows(second).first().getByTestId('body-version-editors')).toContainText(
    owner.username,
  );
  for (const who of [owner, colleague]) {
    await expect(rows(second).nth(1).getByTestId('body-version-editors')).toContainText(
      who.username,
    );
  }

  // The colleague goes back to what they typed.
  await rows(second).nth(1).click();
  await expect(shown(second)).toContainText('Bora adds a line, Ada another');
  await restoreShown(second);
  for (const page of [first, second]) await expect(editor(page).locator('p')).toHaveText(typed);
  // Both go on writing in it.
  await typeAtEnd(first, ', and both agree');
  await expect(editor(second).locator('p')).toHaveText([
    'The first plan',
    'Bora adds a line, Ada another, and both agree',
  ]);
  for (const page of [first, second]) await expect(page.locator('[data-problem]')).toHaveCount(0);
  await first.context().close();
  await second.context().close();
  await api.dispose();
  expect(watched.flat()).toEqual([]);
});

test('a viewer reads the versions and restores none; a reader by the public link is offered none', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('keeper');
  const reader = person('looker');
  await register(base, owner);
  await register(base, reader);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Versions to read',
    cards: [{ title: 'Minutes', status: 'To do' }],
    members: [{ who: reader, role: 'viewer' }],
  });
  const cardId = board.cards.Minutes ?? '';
  await writeWhole(api, board, cardId, 'Decided: ship on Monday');
  await writeWhole(api, board, cardId, 'Decided: ship on Friday');

  const looker = await pageOf(browser, boardPath(board, board.viewId, cardId), reader);
  const errors = watchErrors(looker);
  await openVersions(looker);
  await expect(rows(looker)).toHaveCount(2);
  await rows(looker).nth(1).click();
  await expect(shown(looker)).toContainText('Decided: ship on Monday');
  await expect(looker.getByTestId('body-version-restore')).toHaveCount(0);
  // The server refuses a viewer too.
  const asReader = await signedIn(base, reader);
  const refused = await asReader.post(`${cardAt(board, cardId)}/body-versions/1/restore`);
  expect(refused.status()).toBe(403);
  expect((await kept(asReader, board, cardId)).map((one) => one.version)).toEqual([2, 1]);
  await asReader.dispose();
  await looker.context().close();

  // By the board's public link: the card is read, and its menu has no versions.
  const at = `/api/v1/w/${board.workspaceId}/containers/${board.boardId}`;
  const shared = (await (
    await api.post(`${at}/sharing`, { data: { enabled: true } })
  ).json()) as Schemas['Sharing'];
  const context = await browser.newContext({ locale: 'en-US', baseURL: base });
  const page = await context.newPage();
  const sharedErrors = watchErrors(page);
  await page.goto(
    `/shared/${board.boardId}/${board.viewId}/${cardId}?r=${encodeURIComponent(shared.token ?? '')}`,
  );
  const dialog = page.getByTestId('card-dialog');
  await expect(dialog.getByText('Decided: ship on Friday')).toBeVisible();
  await page.getByTestId('card-menu').click();
  await expect(page.locator('[data-action="copy-link"]')).toBeVisible();
  await expect(page.locator('[data-action="versions"]')).toHaveCount(0);
  // Nor does the server give them to who is signed in as nobody.
  const anonymous = await context.request.get(`${cardAt(board, cardId)}/body-versions`);
  expect(anonymous.status()).toBe(401);
  await context.close();
  await api.dispose();
  expect([...errors, ...sharedErrors]).toEqual([]);
});
