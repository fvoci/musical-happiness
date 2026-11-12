/**
 * The organization on the real server: its administrator makes a workspace on
 * the organization screen, joins it, goes there with the switcher at the head
 * of the sidebar and makes a board. A workspace's administrator opens a board
 * for the invited only that they manage without reading, joins it from there
 * and reads it; another member reads in the workspace's records that it was
 * joined. With `FVOCI_SCREENS` set the screens are pictured under
 * `target/screens/org/`, wide and at 390 pixels.
 */
import { expect, test, type Page } from '@playwright/test';

import { ON_A_VIEW } from '../e2e/address.ts';
import { screensDir } from '../e2e/screens.ts';
import { sidebar } from '../e2e/shell.support.ts';

import {
  ADMIN,
  boardPath,
  invitation,
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  uniqueName,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

/** Pictures the page, wide and narrow, when asked to. */
async function picture(page: Page, name: string): Promise<void> {
  if (!process.env.FVOCI_SCREENS) return;
  const wide = page.viewportSize();
  await page.screenshot({ path: `${screensDir('org')}/${name}-wide.png`, fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: `${screensDir('org')}/${name}-390.png`, fullPage: true });
  if (wide) await page.setViewportSize(wide);
}

test('the organization’s administrator makes a workspace, joins it, switches to it and makes a board', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  await invitation(base);
  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, ADMIN);

  await sidebar(page).getByTestId('open-organization').click();
  const screen = page.getByTestId('org-page');
  await expect(screen).toBeVisible();
  const name = uniqueName('Research');
  await screen.getByTestId('new-workspace-name').fill(name);
  await screen.getByRole('button', { name: 'Create', exact: true }).click();
  const row = screen.locator(`[data-testid="org-workspace"][data-name="${name}"]`);
  await expect(row).toContainText('Members: 0');
  await row.getByRole('button', { name: `Join ${name} as administrator` }).click();
  await expect(row).toContainText('you joined as administrator');
  await expect(row.getByRole('button', { name: `Leave ${name}` })).toBeVisible();
  await expect(screen.getByTestId('org-records').locator('[data-action]').first()).toHaveAttribute(
    'data-action',
    'workspace_created',
  );
  await picture(page, 'organization');

  await screen.getByTestId('org-back').click();
  await expect(page.getByTestId('app-shell')).toBeVisible();
  await sidebar(page).getByTestId('workspace-switcher').click();
  await page.getByRole('menuitem', { name }).click();
  await expect(sidebar(page).getByTestId('workspace-switcher')).toContainText(name);
  await picture(page, 'switcher');

  const made = page.waitForResponse(
    (answer) =>
      /\/api\/v1\/w\/[^/]+\/boards-and-blocks$/.test(answer.url()) && answer.status() === 201,
  );
  await sidebar(page).getByTestId('add-board').click();
  await page.getByTestId('template-picker').getByTestId('empty-board').click();
  await made;
  await expect(page).toHaveURL(ON_A_VIEW);
  const workspaceId = new URL(page.url()).pathname.split('/')[2];
  const api = await signedIn(base, ADMIN);
  const workspaces = (await (await api.get('/api/v1/users/me/workspaces')).json()) as {
    id: string;
    name: string;
  }[];
  expect(workspaces.find((one) => one.id === workspaceId)?.name).toBe(name);
  expect(errors).toEqual([]);
});

test('a board one manages without reading is joined from its page, and the members read that it was', async ({
  page,
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  await invitation(base);
  const owner = person('owner');
  const colleague = person('colleague');
  await register(base, owner);
  await register(base, colleague);
  const board = await makeBoard(await signedIn(base, owner), {
    title: 'Salaries',
    type: 'P',
    cards: [{ title: 'Raise for everyone', status: 'To do' }],
  });

  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId));
  await signInOnPage(page, ADMIN);
  const managed = page.getByTestId('board-managed');
  await expect(managed).toBeVisible();
  await expect(managed.getByRole('heading')).toHaveText('Salaries');
  await expect(managed).toContainText('every member of the workspace will see that you joined');
  await expect(page.getByTestId('board-page')).toHaveCount(0);
  await picture(page, 'managed-board');
  await managed.getByRole('button', { name: 'Join', exact: true }).click();
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page.getByText('Raise for everyone')).toBeVisible();
  expect(errors).toEqual([]);

  const other = await browser.newContext({ locale: 'en-US' });
  const theirs = await other.newPage();
  await theirs.goto(`/w/${board.workspaceId}`);
  await signInOnPage(theirs, colleague);
  await sidebar(theirs).getByTestId('open-settings').click();
  await theirs.getByTestId('open-records').click();
  const records = theirs.getByTestId('workspace-records');
  const joined = records.locator('[data-action="self_added_to_board"]').first();
  await expect(joined).toContainText(`@${ADMIN.username}`);
  // The colleague does not read the board: its title is not told.
  await expect(joined).toContainText('a member of a board');
  await picture(theirs, 'records');
  await other.close();
});
