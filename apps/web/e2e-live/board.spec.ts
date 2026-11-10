/**
 * Boards on the real server: the list of boards in the sidebar, and a board
 * opened from it with its views, cards and members. The board is made
 * through the API by its owner; another person, a member who may only view
 * it, opens it on the page.
 */
import { expect, test } from '@playwright/test';

import { makeBoard, person, register, signedIn, signInOnPage, watchErrors } from './support.ts';

test.use({ locale: 'en-US' });

test('a board of the server is listed and opens with its views, cards and members', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('owner');
  const reader = person('reader');
  await register(base, owner);
  await register(base, reader);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Garden plan',
    cards: [
      { title: 'Dig the beds', status: 'To do' },
      { title: 'Order seeds', status: 'To do' },
      { title: 'Build a fence', status: 'Done' },
    ],
    members: [{ who: reader, role: 'viewer' }],
  });
  await api.dispose();

  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, reader);

  const sidebar = page.getByTestId('sidebar');
  await expect(sidebar.getByText('Garden plan')).toBeVisible();
  await sidebar.getByText('Garden plan').click();

  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`/board/${board.boardId}/${board.viewId}`));
  await expect(page.getByTestId('board-title-text')).toHaveText('Garden plan');
  await expect(
    page.getByRole('navigation', { name: 'Views' }).getByRole('link', { name: 'By status' }),
  ).toBeVisible();

  const column = (name: string) =>
    page.locator('[data-column]').filter({
      has: page.locator('h3').filter({ hasText: new RegExp(`^\\s*${name}\\s*$`) }),
    });
  await expect(column('To do').locator('[data-card]')).toHaveCount(2);
  await expect(column('Done').locator('[data-card]')).toHaveCount(1);
  await expect(column('Done').getByText('Build a fence')).toBeVisible();

  // Both people are members; the reader may only look.
  const members = page.getByTestId('board-members');
  await expect(members.locator('li')).toHaveCount(2);
  // By name, though the server's list of the team is short (ten): the board's
  // members are read by their identifiers.
  await expect(members.getByLabel(owner.username)).toBeVisible();
  await expect(members.getByLabel(reader.username)).toBeVisible();
  await expect(page.getByTestId('read-only')).toBeVisible();

  // A reload opens the same board from its address.
  await page.reload();
  await expect(column('To do').locator('[data-card]')).toHaveCount(2);
  expect(errors).toEqual([]);
});
