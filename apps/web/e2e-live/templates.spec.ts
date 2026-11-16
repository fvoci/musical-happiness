/**
 * Templates on the real server: a board made from a built-in template, a
 * template made from a board and changed on its own page, and the workspace's
 * template offered to be changed again from "Add board".
 */
import { expect, test, type APIRequestContext } from '@playwright/test';

import { boardLink, boardMenu, chooseFromMenu, sidebar } from '../e2e/shell.support.ts';

import {
  boardPath,
  makeBoard,
  person,
  placeOf,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

async function templates(api: APIRequestContext, workspaceId: string) {
  const answer = await api.get(`/api/v1/w/${workspaceId}/templates`);
  expect(answer.ok()).toBe(true);
  return (await answer.json()) as Schemas['Container'][];
}

async function views(api: APIRequestContext, workspaceId: string, boardId: string) {
  return (await (
    await api.get(`/api/v1/w/${workspaceId}/boards/${boardId}/blocks`, { params: { type: 'view' } })
  ).json()) as Schemas['Block'][];
}

test('a board is made from a template, and a template from a board is changed on its page', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('templater');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const mine = await makeBoard(api, { title: 'Weekly plan', cards: [{ title: 'Water' }] });
  const builtIn = (await templates(api, mine.workspaceId)).find(
    (template) => template.title === 'Project Tasks',
  );
  expect(builtIn?.isBuiltin).toBe(true);

  const errors = watchErrors(page);
  await page.goto(boardPath(mine, mine.viewId));
  await signInOnPage(page, owner);

  // "Add board" offers an empty board and the templates; a built-in one is used.
  await sidebar(page).getByTestId('add-board').click();
  const picker = page.getByTestId('template-picker');
  await expect(picker.getByTestId('empty-board')).toBeVisible();
  const projectTasks = picker.locator('[data-template="Project Tasks"]');
  // A built-in template is nobody's to change.
  await expect(projectTasks.getByRole('button', { name: /^Change the template/ })).toHaveCount(0);
  await projectTasks.getByRole('button', { name: 'Use this template: Project Tasks' }).click();
  await expect(boardLink(page, 'Project Tasks')).toHaveAttribute('aria-current', 'page');
  const made = placeOf(page.url()).boardId;
  const copy = (await (
    await api.get(`/api/v1/w/${mine.workspaceId}/containers/${made}`)
  ).json()) as Schemas['Container'];
  // In the reader's workspace (sent as `toWorkspace`: a built-in template is kept apart from every workspace).
  expect(copy).toMatchObject({
    title: 'Project Tasks',
    isTemplate: false,
    type: 'P',
    workspaceId: mine.workspaceId,
  });
  expect((await views(api, mine.workspaceId, made)).length).toBe(
    (await views(api, mine.workspaceId, builtIn?.id ?? '')).length,
  );

  // A template from a board: it opens on its page, and is changed there.
  await chooseFromMenu(page, boardMenu(page, 'Weekly plan'), 'template-from-board');
  await expect(page.getByTestId('template-note')).toBeVisible();
  const title = page.getByTestId('top-bar').getByTestId('board-title');
  await expect(title).toHaveValue('New board template');
  await title.fill('Our weekly plan');
  await title.press('Enter');
  await expect
    .poll(async () =>
      (await templates(api, mine.workspaceId))
        .filter((one) => !one.isBuiltin)
        .map((one) => one.title),
    )
    .toEqual(['Our weekly plan']);
  // No board of the sidebar.
  await expect(boardLink(page, 'Our weekly plan')).toHaveCount(0);
  const templateId = placeOf(page.url()).boardId;

  // Offered again from "Add board", to be changed or used.
  await page.goto(boardPath(mine, mine.viewId));
  await sidebar(page).getByTestId('add-board').click();
  await picker.getByRole('button', { name: 'Change the template Our weekly plan' }).click();
  await expect(page).toHaveURL(new RegExp(boardPath({ ...mine, boardId: templateId })));
  await expect(page.getByTestId('template-note')).toBeVisible();

  await sidebar(page).getByTestId('add-board').click();
  await picker.getByRole('button', { name: 'Use this template: Our weekly plan' }).click();
  await expect(boardLink(page, 'Our weekly plan')).toHaveAttribute('aria-current', 'page');
  await expect(page.getByText('Water')).toBeVisible();
  await expect(page.locator('[data-problem]')).toHaveCount(0);

  await api.dispose();
  expect(errors).toEqual([]);
});
