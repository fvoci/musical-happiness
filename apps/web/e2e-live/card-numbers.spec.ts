/**
 * The numbers of cards on the real server: the server numbers a board's
 * cards from 1 as they are made, the board shows each card's number and an
 * open card shows it at its head; `#` and a number in a body put that card
 * of the board as a chip, the body stored with its identifier and never the
 * number; and the search finds the card by its number on the board shown.
 */
import { expect, test } from '@playwright/test';

import { primaryKey } from '../e2e/keyboard.support.ts';

import {
  boardPath,
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

interface Stored {
  type: string;
  attrs?: Record<string, unknown>;
  content?: Stored[];
}

/** The references of a stored body, whole. */
function references(node: Stored | null): Stored[] {
  if (!node) return [];
  return [
    ...(node.type === 'reference' ? [node] : []),
    ...(node.content ?? []).flatMap(references),
  ];
}

const TITLES = ['Plan the week', 'Fix the login', 'Write the notes'] as const;

test('the board numbers its cards, `#3` in a body puts card 3, and the search finds it by `#3`', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('numberer');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Numbered',
    cards: TITLES.map((title) => ({ title, status: 'To do' })),
  });
  const ids = TITLES.map((title) => board.cards[title] ?? '');
  const [first, , third] = ids;
  // The server numbers them in the order they were made.
  for (const [at, id] of ids.entries()) {
    const card = await api.get(`/api/v1/w/${board.workspaceId}/cards/${id}`);
    expect(((await card.json()) as { number: number | null }).number).toBe(at + 1);
  }

  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId));
  await signInOnPage(page, owner);
  for (const [at, id] of ids.entries()) {
    const face = page.locator(`[data-card="${id}"]`);
    await expect(face.locator('[data-slot="card-number"]')).toHaveAttribute(
      'data-number',
      String(at + 1),
    );
    await expect(face).toHaveAccessibleName(new RegExp(`^number ${String(at + 1)}\\s`));
  }

  await page.goto(boardPath(board, board.viewId, first));
  await expect(page.getByTestId('card-location')).toContainText('#1');
  const body = page.getByTestId('card-body');
  await expect(body).toHaveAttribute('data-editor', 'ready');
  await body.locator('[contenteditable]').first().click();
  await page.keyboard.type('Next: #3');
  const option = page.locator('[data-suggestions="card-number"]').getByRole('option').first();
  await expect(option).toHaveText(/^#3 .*Write the notes$/);
  await page.keyboard.press('Enter');
  const chip = body.locator('[data-reference]');
  await expect(chip).toHaveAttribute('data-state', 'shown');
  await expect(chip).toHaveText(/Write the notes$/);
  await expect
    .poll(async () => {
      const card = await api.get(`/api/v1/w/${board.workspaceId}/cards/${first ?? ''}`);
      return references(((await card.json()) as { body: Stored | null }).body);
    })
    .toEqual([{ type: 'reference', attrs: { id: third } }]);
  const stored = await api.get(`/api/v1/w/${board.workspaceId}/cards/${first ?? ''}`);
  expect(JSON.stringify(((await stored.json()) as { body: unknown }).body)).not.toContain('#3');

  // The search, from the board: `#3` is its card 3, first.
  await page.getByTestId('card-close').click();
  await expect(page.getByTestId('open-search')).toBeVisible();
  await page.keyboard.press(await primaryKey(page, 'k'));
  const search = page.getByTestId('search-dialog');
  await page.getByTestId('search-field').fill('#3');
  await expect(search).toHaveAttribute('data-status', 'found');
  const hit = search.getByRole('option').first();
  await expect(hit).toHaveAttribute('data-search-hit', third ?? '');
  await expect(hit.locator('[data-hit-title]')).toHaveText('Write the notes');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('card-title')).toHaveValue('Write the notes');
  await expect(page.getByTestId('card-location')).toContainText('#3');
  expect(errors).toEqual([]);
  await api.dispose();
});
