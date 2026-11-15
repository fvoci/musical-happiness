/**
 * The search of what cards say, on the real server through the tests' fake
 * search server (`tools/live-server.ts`, nine ports up), which finds by the
 * letters: a card is found by a word of its body and opened from the
 * dialog; another person's private board and a card template are not found;
 * a search server that is down is said so, and searched again once it is up.
 *
 * Pages of wikis are not searched here: the app has no wiki screen yet.
 */
import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

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

const body = (text: string): object => ({
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
});

/** The fake search server beside the live server of `baseURL`. */
async function fake(baseURL: string, route: string): Promise<void> {
  const port = Number(new URL(baseURL).port) + 9;
  // A closed local port can hang here instead of refusing: never wait long.
  const answer = await fetch(`http://127.0.0.1:${port}/_fake/${route}`, {
    method: 'POST',
    signal: AbortSignal.timeout(5000),
  });
  expect(answer.ok, `fake search ${route}`).toBe(true);
}

/** Makes a card with a body on a board, a template of the board where asked. */
async function cardWithBody(
  api: APIRequestContext,
  board: { workspaceId: string; boardId: string },
  title: string,
  text: string,
  isTemplate = false,
): Promise<string> {
  const answer = await api.post(`/api/v1/w/${board.workspaceId}/boards/${board.boardId}/cards`, {
    data: { title, properties: {}, body: body(text), isTemplate },
  });
  expect(answer.ok(), await answer.text()).toBe(true);
  return ((await answer.json()) as Schemas['Card']).id;
}

/** What the server finds for `words`, by identifier: once the cards have reached the index. */
async function foundBy(api: APIRequestContext, workspaceId: string, words: string) {
  const answer = await api.get(`/api/v1/w/${workspaceId}/search`, { params: { q: words } });
  expect(answer.ok(), await answer.text()).toBe(true);
  return ((await answer.json()) as Schemas['SearchHit'][]).map((hit) => hit.id).sort();
}

const dialog = (page: Page) => page.getByTestId('search-dialog');

test('a card is found by a word of its body and opened; what the reader cannot read is not found', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const errors = watchErrors(page);
  const owner = person('searcher');
  const stranger = person('stranger');
  for (const who of [owner, stranger]) await register(base, who);
  const api = await signedIn(base, owner);
  const theirs = await signedIn(base, stranger);
  const word = `quokka${Date.now().toString(36)}`;
  const board = await makeBoard(api, { title: 'Field notes' });
  const card = await cardWithBody(api, board, 'Island survey', `The ${word} count is up`);
  const template = await cardWithBody(api, board, 'Survey template', `A ${word} form`, true);
  const other = await makeBoard(theirs, { title: 'Private notes' });
  const strangers = await cardWithBody(theirs, other, 'Their survey', `Their ${word} too`);

  // Each finds their own card once the server has sent it to the search server,
  // and neither a template nor what is not theirs.
  await expect.poll(() => foundBy(api, board.workspaceId, word)).toEqual([card]);
  await expect.poll(() => foundBy(theirs, other.workspaceId, word)).toEqual([strangers]);
  expect(template).not.toBe(card);

  await page.goto('/');
  await signInOnPage(page, owner);
  await page.keyboard.press('Control+k');
  await page.getByTestId('search-field').fill(word);
  await expect(dialog(page)).toHaveAttribute('data-status', 'found');
  const hits = dialog(page).getByRole('option');
  await expect(hits).toHaveCount(1);
  await expect(hits.first().locator('[data-hit-title]')).toHaveText('Island survey');
  await expect(hits.first().locator('[data-hit-container]')).toHaveText('Field notes');
  await expect(hits.first().locator('mark')).toHaveText(word);
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('card-dialog')).toContainText('Island survey');
  expect(page.url()).toContain(`/boards/${board.boardId}/${board.viewId}/${card}`);
  await api.dispose();
  await theirs.dispose();
  expect(errors).toEqual([]);
});

test('a search server that is down is said so, and searched again once it is up', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const errors = watchErrors(page);
  const owner = person('down-searcher');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const word = `wallaby${Date.now().toString(36)}`;
  const board = await makeBoard(api, { title: 'Outages' });
  const card = await cardWithBody(api, board, 'Postmortem', `The ${word} incident`);
  // Sent before the search server goes down: nothing is written while it is.
  await expect.poll(() => foundBy(api, board.workspaceId, word)).toEqual([card]);

  await page.goto('/');
  await signInOnPage(page, owner);
  await page.getByTestId('open-search').click();
  await fake(base, 'down');
  try {
    await page.getByTestId('search-field').fill(word);
    await expect(dialog(page)).toHaveAttribute('data-status', 'unavailable');
    await expect(dialog(page)).toContainText('Search is not available right now.');
  } finally {
    await fake(base, 'up');
  }
  await page.getByTestId('search-retry').click();
  await expect(dialog(page)).toHaveAttribute('data-status', 'found');
  await expect(dialog(page).getByRole('option')).toHaveCount(1);
  await api.dispose();
  expect(errors).toEqual([]);
});
