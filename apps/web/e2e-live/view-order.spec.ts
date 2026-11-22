/**
 * The order of a board's views on the real server: a view moved by the keys
 * alone in the list of views, seen in the list of another member who has the
 * board open, there again after a reload, and kept by a copy of the board
 * and by a board made from it as a template. A member who may only look has
 * nothing to move them with.
 */
import { expect, test, type Page } from '@playwright/test';

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

test.use({ locale: 'en-US', viewport: { width: 1400, height: 900 } });

const viewsButton = (page: Page) => page.locator('[data-tool="views"]');
const listed = (page: Page) =>
  page.getByRole('menu', { name: 'Views' }).locator('a[role="menuitem"]');

/**
 * The titles of views in the board's order: by place (`fields.order`, as
 * strings compare), the identifier settling two of one place; those without
 * a place after them, by title. A space sorts before every digit of a place.
 */
function inOrder(views: Schemas['Block'][]): string[] {
  const key = (view: Schemas['Block']): string => {
    const place = (view.fields as { order?: unknown }).order;
    return typeof place === 'string' ? `0${place} ${view.id}` : `1${view.title}`;
  };
  return [...views]
    .sort((one, other) => (key(one) < key(other) ? -1 : 1))
    .map((view) => view.title);
}

test('a view moved by the keys is the board’s order for another member, after a reload and in copies', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('view-mover');
  const helper = person('view-watcher');
  const reader = person('view-reader');
  for (const who of [owner, helper, reader]) await register(base, who);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Views in order',
    members: [
      { who: helper, role: 'editor' },
      { who: reader, role: 'viewer' },
    ],
  });
  const ws = board.workspaceId;
  const viewsOf = async (boardId: string): Promise<Schemas['Block'][]> =>
    (
      (await (
        await api.get(`/api/v1/w/${ws}/boards/${boardId}/blocks`)
      ).json()) as Schemas['Block'][]
    ).filter((block) => block.type === 'view');
  // Three more views beside "By status", like it. Nobody has moved one: they go by title.
  const [first] = await viewsOf(board.boardId);
  const added = await api.post(`/api/v1/w/${ws}/boards/${board.boardId}/blocks`, {
    data: ['Timeline', 'Backlog', 'Roadmap'].map((title) => ({
      containerId: board.boardId,
      type: 'view',
      title,
      createAt: 1,
      updateAt: 1,
      fields: first?.fields,
    })),
  });
  expect(added.ok(), await added.text()).toBe(true);
  const before = ['Backlog', 'By status', 'Roadmap', 'Timeline'];

  const pages: Page[] = [];
  const watched: string[][] = [];
  for (const who of [owner, helper, reader]) {
    const page = await (await browser.newContext({ locale: 'en-US' })).newPage();
    pages.push(page);
    watched.push(watchErrors(page));
    await page.goto(boardPath(board, board.viewId));
    await signInOnPage(page, who);
    await expect(viewsButton(page)).toHaveText('By status');
  }
  const [mine, theirs, looking] = pages as [Page, Page, Page];

  // The member who may only look has the list, and nothing that moves a view.
  await viewsButton(looking).click();
  await expect(listed(looking)).toHaveText(before);
  await expect(looking.locator('[data-line-tool]')).toHaveCount(0);
  await looking.keyboard.press('Escape');

  // Two places down by the keys, Alt and an arrow on its line: the list open, the focus there.
  await viewsButton(mine).focus();
  await mine.keyboard.press('Enter');
  await expect(listed(mine)).toHaveText(before);
  const backlog = listed(mine).filter({ hasText: 'Backlog' });
  await expect(backlog).toBeFocused();
  await mine.keyboard.press('Alt+ArrowDown');
  await expect(listed(mine).nth(1)).toHaveText('Backlog');
  await mine.keyboard.press('Alt+ArrowDown');
  const moved = ['By status', 'Roadmap', 'Backlog', 'Timeline'];
  await expect(listed(mine)).toHaveText(moved);
  await expect(backlog).toBeFocused();
  await expect(mine.getByText('Backlog is now 3 of 4')).toBeAttached();
  await mine.keyboard.press('Escape');
  await expect(viewsButton(mine)).toBeFocused();

  // The other member's list follows, and so does the reader's.
  await viewsButton(theirs).click();
  await expect(listed(theirs)).toHaveText(moved);
  await theirs.keyboard.press('Escape');
  await viewsButton(looking).click();
  await expect(listed(looking)).toHaveText(moved);
  await looking.keyboard.press('Escape');

  // Every view has its place on the server, and the page read anew lists them so.
  await expect.poll(async () => inOrder(await viewsOf(board.boardId))).toEqual(moved);
  await mine.reload();
  await viewsButton(mine).click();
  await expect(listed(mine)).toHaveText(moved);
  await mine.keyboard.press('Escape');

  // A copy of the board keeps the order, and so does a board made from it as a template.
  const copyOf = async (containerId: string, params: Record<string, string | boolean>) => {
    const copied = await api.post(`/api/v1/w/${ws}/containers/${containerId}/duplicate`, {
      params,
    });
    expect(copied.ok(), await copied.text()).toBe(true);
    return ((await copied.json()) as Schemas['ContainersAndBlocks']).containers[0]?.id ?? '';
  };
  const copyId = await copyOf(board.boardId, {});
  expect(inOrder(await viewsOf(copyId))).toEqual(moved);
  const templateId = await copyOf(board.boardId, { asTemplate: true });
  const fromTemplate = await copyOf(templateId, { toWorkspace: ws });
  expect(inOrder(await viewsOf(fromTemplate))).toEqual(moved);
  await mine.goto(boardPath({ workspaceId: ws, boardId: fromTemplate }));
  await viewsButton(mine).click();
  await expect(listed(mine)).toHaveText(moved);

  for (const errors of watched) expect(errors).toEqual([]);
  for (const page of pages) await page.context().close();
  await api.dispose();
});
