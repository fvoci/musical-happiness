/**
 * An open board on the real server, opened by somebody of the workspace who is
 * no member of it: it is read as it is, under a bar that joins it, as an open
 * wiki is, and joining lets them write in it. Nobody is made a member by
 * opening it.
 */
import { expect, test } from '@playwright/test';

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

test('an open board is joined from its page, not by opening it', async ({ page, baseURL }) => {
  const base = baseURL ?? '';
  const owner = person('opener');
  const visitor = person('visitor');
  await register(base, owner);
  await register(base, visitor);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Commons',
    visibility: 'open',
    description: 'For the whole workspace',
    cards: [{ title: 'Water the plants', status: 'To do' }],
  });
  const memberNames = async (): Promise<string[]> => {
    const members = (await (
      await api.get(`/api/v1/w/${board.workspaceId}/containers/${board.boardId}/members`)
    ).json()) as Schemas['ContainerMember'][];
    const users = (await (
      await api.post(`/api/v1/w/${board.workspaceId}/users`, {
        data: members.map((member) => member.userId),
      })
    ).json()) as Schemas['User'][];
    return users.map((user) => user.username).sort();
  };

  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId));
  await signInOnPage(page, visitor);
  const join = page.getByTestId('board-join');
  await expect(join).toBeVisible();
  await expect(join).toContainText('You read this board without being a member');
  // The board is read as it is, its cards with it.
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page.getByText('Water the plants')).toBeVisible();
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  // Opening it made nobody a member.
  expect(await memberNames()).toEqual([owner.username]);

  await join.getByRole('button', { name: 'Join this board' }).click();
  await expect(join).toHaveCount(0);
  // The focus is on the board, where the button was.
  await expect(page.getByTestId('board-page')).toBeFocused();
  await expect(page.getByText('Water the plants')).toBeVisible();
  expect(await memberNames()).toEqual([owner.username, visitor.username].sort());

  await page.reload();
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  await api.dispose();
  expect(errors).toEqual([]);
});
