/**
 * A card relation on the real server: the server takes a property definition
 * of the kind `relation` and its value as a list of card identifiers, and the
 * value picked in the card is there after the page is loaded again.
 *
 * The definition is made through the API: the app does not send changes of a
 * board's definitions to the server yet (`patchBoard` is not wired).
 */
import { expect, test, type Page } from '@playwright/test';

import {
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US', timezoneId: 'UTC' });

function property(page: Page, name: string) {
  return page
    .getByTestId('card-dialog')
    .locator('[data-property]')
    .filter({ has: page.locator('dt', { hasText: new RegExp(`^\\s*${name}\\s*$`) }) });
}

async function closePicker(page: Page): Promise<void> {
  const sheet = page.locator('[data-testid="bottom-sheet"][data-state="open"]');
  if ((await sheet.count()) > 0) await sheet.last().getByRole('button', { name: 'Done' }).click();
  else await page.keyboard.press('Escape');
  await expect(sheet).toHaveCount(0);
}

test('cards picked into a relation are stored as a list of identifiers and stay after a reload', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('relater');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Relations',
    cards: [
      { title: 'Pour the foundation', status: 'To do' },
      { title: 'Order the bricks', status: 'To do' },
      { title: 'Build the walls', status: 'To do' },
    ],
  });
  const defined = await api.patch(`/api/v1/boards/${board.boardId}`, {
    data: {
      updatedCardProperties: [{ id: 'after', name: 'After', type: 'relation', options: [] }],
    },
  });
  expect(defined.ok(), await defined.text()).toBe(true);

  const walls = board.cards['Build the walls'] ?? '';
  const foundation = board.cards['Pour the foundation'] ?? '';
  const bricks = board.cards['Order the bricks'] ?? '';
  const values = async (): Promise<Schemas['Card']['properties']> =>
    ((await (await api.get(`/api/v1/cards/${walls}`)).json()) as Schemas['Card']).properties;

  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${board.viewId}/${walls}`);
  await signInOnPage(page, owner);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  const after = property(page, 'After');
  await expect(after).toHaveAttribute('data-property-type', 'relation');

  for (const title of ['Pour the foundation', 'Order the bricks']) {
    await after.locator('[data-slot="relation-edit"]').click();
    const search = page.getByRole('combobox', { name: 'After' });
    await search.fill(title);
    await expect(page.getByRole('listbox', { name: 'After' }).getByRole('option')).toHaveCount(1);
    await search.press('Enter');
    await closePicker(page);
  }
  await expect.poll(async () => (await values()).after).toEqual([foundation, bricks]);

  await page.reload();
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(after.locator('[data-slot="relation-chip"]')).toHaveText([
    'Pour the foundation',
    'Order the bricks',
  ]);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  await api.dispose();
  expect(errors).toEqual([]);
});
