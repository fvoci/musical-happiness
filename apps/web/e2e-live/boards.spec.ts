/**
 * Boards made and changed on the page, on the real server: "Add board"
 * makes the board and its first view in one request, its title and a new
 * view are kept, and a deleted board comes back with undo.
 */
import { expect, test, type APIRequestContext } from '@playwright/test';

import { ON_A_VIEW } from '../e2e/address.ts';
import { boardLink, boardMenu, chooseFromMenu, sidebar } from '../e2e/shell.support.ts';

import {
  person,
  placeOf,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

async function views(api: APIRequestContext, boardId: string): Promise<Schemas['Block'][]> {
  const answer = await api.get(`/api/v1/boards/${boardId}/blocks`, { params: { type: 'view' } });
  expect(answer.ok()).toBe(true);
  return (await answer.json()) as Schemas['Block'][];
}

test('a board is made with its first view, changed, deleted and brought back', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('maker');
  await register(base, owner);
  const api = await signedIn(base, owner);

  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, owner);

  // "Add board", an empty one: the board and a kanban grouped by its "Status", made together.
  const made = page.waitForResponse(
    (answer) => answer.url().endsWith('/api/v1/boards-and-blocks') && answer.status() === 201,
  );
  await sidebar(page).getByTestId('add-board').click();
  await page.getByTestId('template-picker').getByTestId('empty-board').click();
  await made;
  await expect(page).toHaveURL(ON_A_VIEW);
  const { boardId, viewId } = placeOf(page.url());
  const board = (await (await api.get(`/api/v1/boards/${boardId}`)).json()) as Schemas['Board'];
  const status = board.cardProperties.find((template) => template.type === 'select');
  expect(status?.name).toBe('Status');
  const [first, ...others] = await views(api, boardId);
  expect(others).toEqual([]);
  expect(first).toMatchObject({ id: viewId, title: 'Board view' });
  expect(first?.fields).toMatchObject({ viewType: 'board', groupById: status?.id });
  // Filed under the reader's default category by the server.
  const categories = (await (
    await api.get(`/api/v1/w/${board.workspaceId}/categories`)
  ).json()) as Schemas['CategoryBoards'][];
  expect(categories.flatMap((one) => one.boardMetadata.map((entry) => entry.boardId))).toContain(
    boardId,
  );

  // Its title.
  const title = page.getByTestId('top-bar').getByTestId('board-title');
  await title.fill('Launch plan');
  await title.press('Enter');
  await expect(boardLink(page, 'Launch plan')).toBeVisible();
  await expect
    .poll(
      async () =>
        ((await (await api.get(`/api/v1/boards/${boardId}`)).json()) as Schemas['Board']).title,
    )
    .toBe('Launch plan');

  // A second view.
  await page.getByTestId('view-header').locator('[data-tool="add-view"]').click();
  await page.getByRole('menuitem', { name: 'Table', exact: true }).click();
  await expect(page).not.toHaveURL(new RegExp(`/${viewId}$`));
  await expect
    .poll(async () => (await views(api, boardId)).map((view) => view.fields.viewType).sort())
    .toEqual(['board', 'table']);

  // Deleted: gone from the server; undo brings it back.
  await chooseFromMenu(page, boardMenu(page, 'Launch plan'), 'delete');
  await page.getByRole('alertdialog').getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(boardLink(page, 'Launch plan')).toHaveCount(0);
  await expect.poll(async () => (await api.get(`/api/v1/boards/${boardId}`)).status()).toBe(404);
  await expect(page.getByTestId('not-found')).toBeVisible();
  await page.getByTestId('undo').click();
  await expect(boardLink(page, 'Launch plan')).toBeVisible();
  // The page, which showed it as gone, shows it again.
  await expect(title).toHaveValue('Launch plan');
  await expect(page.getByTestId('not-found')).toHaveCount(0);
  expect((await api.get(`/api/v1/boards/${boardId}`)).status()).toBe(200);
  expect(await views(api, boardId)).toHaveLength(2);

  // Brought back, it is filed nowhere; hidden, it is filed first and stays hidden.
  await chooseFromMenu(page, boardMenu(page, 'Launch plan'), 'hide');
  await expect
    .poll(async () => {
      const categories = (await (
        await api.get(`/api/v1/w/${board.workspaceId}/categories`)
      ).json()) as Schemas['CategoryBoards'][];
      return categories
        .flatMap((one) => one.boardMetadata)
        .find((entry) => entry.boardId === boardId);
    })
    .toEqual({ boardId, hidden: true });
  await page.waitForTimeout(500);
  await expect(boardLink(page, 'Launch plan')).toHaveCount(0);
  await expect(page.locator('[data-problem]')).toHaveCount(0);

  await api.dispose();
  expect(errors).toEqual([]);
});
