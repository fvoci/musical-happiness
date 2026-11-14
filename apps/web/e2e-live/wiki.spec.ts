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

  // Read anew at the child's address: the same tree, the child shown, its parent open.
  await showTree(page);
  await treeItem(page, 'Getting started').click();
  await expect(page).toHaveURL(`/w/${workspaceId}/wikis/${wikiId}/${child}`);
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
