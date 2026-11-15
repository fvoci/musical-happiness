/**
 * Wikis on the real server: made from "New" (`createWiki`), filed in the
 * sidebar among the boards, at an address of their own that the board's
 * screen does not open, nor the wiki's a board; and their pages, made in a
 * tree, named and given an icon, there again when the page is read anew.
 */
import { expect, test } from '@playwright/test';

import { sidebar } from '../e2e/shell.support.ts';
import {
  nameThePage,
  newWiki,
  pageAction,
  pageBody,
  hideTree,
  showTree,
  treeItem,
  treeShape,
  wikiPlaceOf,
  wikiScreen,
} from '../e2e/wiki.support.ts';

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

test('a wiki is made, filed among the boards, and kept apart from them', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('wiki-maker');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, { title: 'Roadmap' });

  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, owner);
  const wikiId = await newWiki(page, 'Handbook');
  const { workspaceId } = board;
  await expect(page).toHaveURL(`/w/${workspaceId}/wikis/${wikiId}`);
  await expect(page.getByTestId('wiki-empty')).toBeVisible();

  // The server has it as a wiki, filed in the default category, and no board.
  const wikis = (await (
    await api.get(`/api/v1/w/${workspaceId}/wikis`)
  ).json()) as Schemas['Board'][];
  expect(wikis.map((wiki) => [wiki.id, wiki.kind, wiki.title, wiki.type])).toEqual([
    [wikiId, 'wiki', 'Handbook', 'O'],
  ]);
  const boards = (await (
    await api.get(`/api/v1/w/${workspaceId}/boards`)
  ).json()) as Schemas['Board'][];
  expect(boards.map((one) => one.id)).not.toContain(wikiId);
  const categories = (await (
    await api.get(`/api/v1/w/${workspaceId}/categories`)
  ).json()) as Schemas['CategoryBoards'][];
  const system = categories.find((category) => category.type === 'system');
  expect(system?.boardMetadata.map((entry) => entry.boardId)).toContain(wikiId);

  // The sidebar has both, the wiki said to be one; read again, it still has.
  await page.reload();
  await expect(wikiScreen(page)).toBeVisible();
  await expect(sidebar(page).getByRole('link', { name: 'Handbook, wiki' })).toHaveAttribute(
    'aria-current',
    'page',
  );
  await expect(sidebar(page).getByRole('link', { name: 'Roadmap', exact: true })).toBeVisible();

  // A wiki at a board's address is not there, nor a board at a wiki's.
  await page.goto(boardPath({ workspaceId, boardId: wikiId }));
  await expect(page.getByTestId('not-found')).toBeVisible();
  await page.goto(`/w/${workspaceId}/wikis/${board.boardId}`);
  await expect(page.getByTestId('not-found')).toBeVisible();
  await page.goto(boardPath(board));
  await expect(page.getByTestId('board-page')).toBeVisible();
  expect(errors).toEqual([]);
  await api.dispose();
});

test('the pages of a wiki are made as a tree, named, and there again after a reload', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('wiki-pages');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const { workspaceId } = await makeBoard(api, { title: 'Roadmap' });

  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, owner);
  const wikiId = await newWiki(page, 'Handbook');
  await page.getByTestId('wiki-first-page').click();
  await nameThePage(page, 'Welcome');
  const welcome = wikiPlaceOf(page.url()).pageId;
  await page.getByTestId('page-add-icon').click();
  await expect(page.getByTestId('page-icon')).toBeVisible();
  await pageAction(page, 'Welcome', 'add-child');
  await nameThePage(page, 'Getting started');
  const child = wikiPlaceOf(page.url()).pageId;
  await pageAction(page, 'Welcome', 'add-after');
  await nameThePage(page, 'Policies');

  // The server keeps them as the tree shows them.
  const pages = (await (
    await api.get(`/api/v1/w/${workspaceId}/wikis/${wikiId}/pages`)
  ).json()) as Schemas['PageNode'][];
  const parentOf = new Map(pages.map((one) => [one.title, one.parentId]));
  expect(parentOf.get('Getting started')).toBe(welcome);
  expect(pages.find((one) => one.id === welcome)?.icon).not.toBe('');

  // Read anew at the child's address, its parent folded before: the same tree, the
  // child shown, and its parent open again, for the page shown is in sight.
  await showTree(page);
  await treeItem(page, 'Getting started').click();
  await expect(page).toHaveURL(`/w/${workspaceId}/wikis/${wikiId}/${child}`);
  await showTree(page);
  await treeItem(page, 'Welcome').getByRole('button', { name: 'Fold Welcome' }).click();
  await expect(treeItem(page, 'Getting started')).toHaveCount(0);
  await page.reload();
  await expect(page).toHaveURL(`/w/${workspaceId}/wikis/${wikiId}/${child}`);
  await expect(page.getByTestId('page-title')).toHaveValue('Getting started');
  await showTree(page);
  expect(await treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies']);
  await expect(treeItem(page, 'Getting started')).toHaveAttribute('aria-selected', 'true');
  await expect(treeItem(page, 'Welcome').getByTestId('tree-icon')).not.toBeEmpty();

  // The wiki's address alone shows its first page.
  await page.goto(`/w/${workspaceId}/wikis/${wikiId}`);
  await expect(page).toHaveURL(`/w/${workspaceId}/wikis/${wikiId}/${welcome}`);
  expect(errors).toEqual([]);
  await api.dispose();
});

test('two people write one page together, see each other on it, and the tree follows a deletion', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('wiki-ada');
  const colleague = person('wiki-bora');
  await register(base, owner);
  await register(base, colleague);
  const api = await signedIn(base, owner);
  const { workspaceId } = await makeBoard(api, { title: 'Roadmap' });
  const inWorkspace = `/api/v1/w/${workspaceId}`;
  const wiki = (await (
    await api.post(`${inWorkspace}/wikis`, { data: { title: 'Handbook', type: 'P' } })
  ).json()) as Schemas['Board'];
  const [found] = (await (
    await api.get(`${inWorkspace}/users`, { params: { search: colleague.username } })
  ).json()) as Schemas['User'][];
  await api.post(`${inWorkspace}/boards/${wiki.id}/members`, {
    data: { userId: found?.id, schemeViewer: true, schemeCommenter: true, schemeEditor: true },
  });
  const pageOf = async (title: string, parentId?: string): Promise<string> =>
    (
      (await (
        await api.post(`${inWorkspace}/wikis/${wiki.id}/pages`, {
          data: { title, ...(parentId ? { parentId } : {}) },
        })
      ).json()) as Schemas['Page']
    ).id;
  const welcome = await pageOf('Welcome');
  await pageOf('Below', welcome);
  const other = await pageOf('Other');
  const path = `/w/${workspaceId}/wikis/${wiki.id}/${welcome}`;

  const [first, second] = await Promise.all(
    [owner, colleague].map(async (who) => {
      const page = await (await browser.newContext({ locale: 'en-US' })).newPage();
      await page.goto(path);
      await signInOnPage(page, who);
      await expect(page.getByTestId('page-body')).toHaveAttribute('data-editor', 'ready');
      return page;
    }),
  );
  if (!first || !second) throw new Error('two pages');
  const watched = [watchErrors(first), watchErrors(second)];

  // What one types the other sees, and the other way round.
  await pageBody(first).click();
  await first.keyboard.type('Ada writes.', { delay: 15 });
  await expect(pageBody(second)).toHaveText('Ada writes.');
  await pageBody(second).click();
  await second.keyboard.press('End');
  await second.keyboard.type(' Bora adds.', { delay: 15 });
  for (const page of [first, second]) {
    await expect(pageBody(page)).toHaveText('Ada writes. Bora adds.');
    // The other is on the page, at its head.
    await expect(page.getByTestId('page-heading').getByTestId('page-viewers')).toBeVisible();
  }
  // The server has the body, and both read it again as it was.
  await expect
    .poll(async () => {
      const stored = (await (
        await api.get(`${inWorkspace}/wikis/${wiki.id}/pages/${welcome}`)
      ).json()) as Schemas['Page'];
      return JSON.stringify(stored.body);
    })
    .toContain('Bora adds.');
  await second.reload();
  await expect(pageBody(second)).toHaveText('Ada writes. Bora adds.');

  // A page renamed elsewhere is renamed in the tree.
  await api.patch(`${inWorkspace}/wikis/${wiki.id}/pages/${other}`, {
    data: { title: 'Elsewhere' },
  });
  await showTree(second);
  await expect(treeItem(second, 'Elsewhere')).toBeVisible();
  await hideTree(second);

  // The first deletes the page, asked first and told the page below goes too: it and
  // the page below it leave the second's tree, and the second is told and moved on.
  await pageAction(first, 'Welcome', 'delete');
  const asked = first.getByTestId('confirm-dialog');
  await expect(asked).toContainText('(1)');
  await asked.getByRole('button', { name: 'Delete' }).click();
  await showTree(first);
  expect(await treeShape(first)).toEqual(['Elsewhere']);
  await expect(second.getByTestId('wiki-notice')).toContainText('Welcome');
  await expect(second).toHaveURL(`/w/${workspaceId}/wikis/${wiki.id}/${other}`);
  await expect(second.getByTestId('page-title')).toHaveValue('Elsewhere');
  await showTree(second);
  expect(await treeShape(second)).toEqual(['Elsewhere']);
  for (const errors of watched) expect(errors).toEqual([]);
  await first.context().close();
  await second.context().close();
  await api.dispose();
});

test('a wiki is renamed and described, deleted from its menu, and restored from the trash', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('wiki-keeper');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const { workspaceId } = await makeBoard(api, { title: 'Roadmap' });
  const inWorkspace = `/api/v1/w/${workspaceId}`;
  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, owner);
  const wikiId = await newWiki(page, 'Handbook');
  await page.getByTestId('wiki-first-page').click();
  await nameThePage(page, 'Welcome');

  await showTree(page);
  await page.getByTestId('wiki-menu').click();
  await page.locator('[data-action="edit-wiki"]').click();
  const settings = page.getByTestId('wiki-settings');
  await settings.getByTestId('wiki-settings-name').fill('Guide');
  await settings.getByTestId('wiki-settings-description').fill('How we work.');
  await settings.getByTestId('wiki-settings-save').click();
  await expect(page.getByTestId('wiki-header-title')).toHaveText('Guide');
  await expect
    .poll(async () => {
      const wikis = (await (await api.get(`${inWorkspace}/wikis`)).json()) as Schemas['Board'][];
      const found = wikis.find((one) => one.id === wikiId);
      return [found?.title, found?.description];
    })
    .toEqual(['Guide', 'How we work.']);

  await page.getByTestId('wiki-menu').click();
  await page.locator('[data-action="delete-wiki"]').click();
  await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Delete' }).click();
  await expect(wikiScreen(page)).toHaveCount(0);
  await sidebar(page).getByRole('button', { name: 'Trash' }).click();
  const item = page.getByTestId('board-trash').locator('li', { hasText: 'Guide' });
  await expect(item.getByTestId('trash-kind')).toHaveText('wiki');
  await item.getByRole('button', { name: /^Restore/ }).click();
  await page.keyboard.press('Escape');
  await sidebar(page).getByRole('link', { name: 'Guide, wiki' }).click();
  await expect(page.getByTestId('page-title')).toHaveValue('Welcome');
  expect(errors).toEqual([]);
  await api.dispose();
});

test('an open wiki is listed to whoever works in the workspace, joined first, then read', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('wiki-opener');
  const colleague = person('wiki-joiner');
  await register(base, owner);
  await register(base, colleague);
  const api = await signedIn(base, owner);
  const { workspaceId } = await makeBoard(api, { title: 'Roadmap' });
  const inWorkspace = `/api/v1/w/${workspaceId}`;
  const wiki = (await (
    await api.post(`${inWorkspace}/wikis`, { data: { title: 'Open notes', type: 'O' } })
  ).json()) as Schemas['Board'];
  const first = (await (
    await api.post(`${inWorkspace}/wikis/${wiki.id}/pages`, { data: { title: 'Hello' } })
  ).json()) as Schemas['Page'];

  const page = await (await browser.newContext({ locale: 'en-US' })).newPage();
  const errors = watchErrors(page);
  await page.goto(`/w/${workspaceId}/wikis/${wiki.id}`);
  await signInOnPage(page, colleague);
  // Listed to them, its pages not theirs to read until they join.
  await expect(sidebar(page).getByRole('link', { name: 'Open notes, wiki' })).toBeVisible();
  const join = page.getByTestId('wiki-join');
  await expect(join).toContainText('Open notes');
  await join.getByRole('button').click();
  await expect(page).toHaveURL(`/w/${workspaceId}/wikis/${wiki.id}/${first.id}`);
  await expect(page.getByTestId('page-title')).toHaveValue('Hello');
  expect(errors).toEqual([]);
  await page.context().close();
  await api.dispose();
});
