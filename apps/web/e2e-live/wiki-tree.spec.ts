/**
 * The tree of a wiki on the real server: pages moved with the keyboard, from
 * "Move…" and dragged, there again where they were put after a reload, as
 * the server keeps them (`movePage`).
 */
import { expect, test, type Page } from '@playwright/test';

import { nextFrame, pointOn } from '../e2e/kanban.support.ts';
import {
  nameThePage,
  newWiki,
  pageAction,
  pageTree,
  showTree,
  treeItem,
  treeShape,
} from '../e2e/wiki.support.ts';

import {
  makeBoard,
  person,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

const row = (page: Page, name: string) =>
  pageTree(page).locator(`[data-page][aria-label="${name}"]`);

test('pages moved with the keys, from "Move…" and dragged are where they were put after a reload', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('wiki-moves');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const { workspaceId } = await makeBoard(api, { title: 'Roadmap' });

  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, owner);
  const wikiId = await newWiki(page, 'Handbook');
  await page.getByTestId('wiki-first-page').click();
  await nameThePage(page, 'Welcome');
  await pageAction(page, 'Welcome', 'add-child');
  await nameThePage(page, 'Getting started');
  await pageAction(page, 'Welcome', 'add-after');
  await nameThePage(page, 'Policies');
  await pageAction(page, 'Policies', 'add-after');
  await nameThePage(page, 'Travel');
  await showTree(page);

  // Up among its siblings with the keyboard, into "Policies" from "Move…", and
  // "Getting started" dragged before "Welcome".
  await treeItem(page, 'Travel').focus();
  await page.keyboard.press('Alt+ArrowUp');
  await expect
    .poll(() => treeShape(page))
    .toEqual(['Welcome', '  Getting started', 'Travel', 'Policies']);
  await pageAction(page, 'Welcome', 'move');
  const dialog = page.getByTestId('move-page');
  await dialog.getByRole('radio', { name: 'Policies, level 1' }).check();
  await dialog.getByRole('button', { name: 'Move', exact: true }).click();
  // Gone with its backdrop, which would take the press of the drag.
  await expect(dialog).toBeHidden();
  await expect(page.locator('[data-slot="dialog-overlay"]')).toHaveCount(0);
  await showTree(page);
  await expect
    .poll(() => treeShape(page))
    .toEqual(['Travel', 'Policies', '  Welcome', '    Getting started']);
  const from = await pointOn(row(page, 'Getting started'));
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 4, from.y - 6, { steps: 3 });
  const above = await pointOn(row(page, 'Travel'), 0.1);
  await page.mouse.move(above.x, above.y, { steps: 6 });
  await nextFrame(page);
  await expect(row(page, 'Travel')).toHaveAttribute('data-drop', 'before');
  await page.mouse.up();
  const moved = ['Getting started', 'Travel', 'Policies', '  Welcome'];
  await expect.poll(() => treeShape(page)).toEqual(moved);

  // The server keeps them so, and the page read anew shows them so.
  const pages = (await (
    await api.get(`/api/v1/w/${workspaceId}/wikis/${wikiId}/pages`)
  ).json()) as Schemas['PageNode'][];
  const top = pages.filter((one) => one.parentId === '').map((one) => one.title);
  expect(top).toEqual(['Getting started', 'Travel', 'Policies']);
  await page.reload();
  await showTree(page);
  await expect.poll(() => treeShape(page)).toEqual(moved);
  expect(errors).toEqual([]);
  await api.dispose();
});
