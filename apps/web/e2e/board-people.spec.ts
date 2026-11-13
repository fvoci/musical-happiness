/**
 * The head of a board, on the in-memory backend: the faces of who is here now and the
 * number of the board's members, which opens the members of the sharing dialog. Others
 * come onto the board as other users of the same world (`fvociDev.backend.as`).
 */
import { expect, test, type Page } from '@playwright/test';

import { boardLink, openApp, READ_ONLY_BOARD, showSidebar, topBar } from './shell.support.ts';

interface Dev {
  workspace: {
    me?: { id: string };
    board: {
      boardId: string;
      board: { workspaceId: string };
      members: { userId: string }[];
      addMember(userId: string, role: string): Promise<void>;
    };
  };
  backend: {
    world: { users: Map<string, unknown> };
    as(userId: string): {
      presence: { setPresence(visit: { workspaceId: string; boardId: string } | null): void };
    };
  };
}

/**
 * Makes `count` other users members of the open board and has them come onto it; the
 * number of its members then.
 */
async function othersArrive(page: Page, count: number): Promise<number> {
  return page.evaluate(async (wanted) => {
    const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
    const store = dev.workspace.board;
    const others = [...dev.backend.world.users.keys()]
      .filter((id) => id !== dev.workspace.me?.id)
      .slice(0, wanted);
    for (const id of others) {
      if (!store.members.some((member) => member.userId === id))
        await store.addMember(id, 'editor');
    }
    // Kept on the page: a visit lasts as long as its presence is not let go of.
    const visits = others.map((id) => dev.backend.as(id).presence);
    for (const presence of visits)
      presence.setPresence({ workspaceId: store.board.workspaceId, boardId: store.boardId });
    Object.assign(window, { visits });
    return store.members.length;
  }, count);
}

const people = (page: Page) => topBar(page).getByTestId('board-people');
const faces = (page: Page) =>
  topBar(page).getByTestId('board-presence').locator('[data-avatar]:visible');

test('alone, the head of the board shows nobody here and the number of members', async ({
  page,
}) => {
  await openApp(page);
  const members = await page.evaluate(
    () => (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board.members.length,
  );
  await expect(topBar(page).getByTestId('board-members-count')).toHaveText(`Members ${members}`);
  await expect(people(page)).toHaveAttribute('aria-label', `Viewing now: 0, members: ${members}`);
  await expect(faces(page)).toHaveCount(0);
});

test('five faces in a wide pane and two in a narrow one, then the rest as a number', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openApp(page);
  const members = await othersArrive(page, 3);
  const presence = topBar(page).getByTestId('board-presence');
  await expect(presence).toHaveAccessibleName('People viewing this board: 3');
  await expect(people(page)).toHaveAttribute('aria-label', `Viewing now: 3, members: ${members}`);
  await expect(faces(page)).toHaveCount(3);
  // "+k" shows only when there are more than the faces shown.
  const plus = presence.locator('span:visible', { hasText: '+' });
  await expect(plus).toHaveCount(0);
  await expect(topBar(page).getByTestId('board-members-count')).toHaveText(`Members ${members}`);

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(faces(page)).toHaveCount(2);
  await expect(plus).toHaveText('+1');
  await expect(topBar(page).getByTestId('board-members-count')).toBeVisible();
});

test('the number of members opens the sharing dialog at its members', async ({ page }) => {
  await openApp(page);
  await topBar(page).getByTestId('board-members-count').click();
  const dialog = page.getByTestId('share-dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId('share-members-title')).toBeFocused();
  // An admin changes them here: the search to add people is there.
  await expect(dialog.getByTestId('share-search')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  // The share button opens the same dialog at its top.
  await topBar(page).getByTestId('share-board').click();
  await expect(dialog.getByTestId('share-members-title')).not.toBeFocused();
  await expect(dialog.getByTestId('share-open')).toBeVisible();
});

test('a reader who is no admin sees the members and changes nothing', async ({ page }) => {
  await openApp(page);
  await showSidebar(page);
  await boardLink(page, READ_ONLY_BOARD).click();
  await expect(topBar(page).getByTestId('read-only')).toBeVisible();
  await expect(topBar(page).getByTestId('share-board')).toHaveCount(0);
  await topBar(page).getByTestId('board-members-count').click();
  const dialog = page.getByTestId('share-dialog');
  await expect(dialog.getByTestId('share-members-title')).toBeFocused();
  await expect(dialog.getByTestId('share-member-role').first()).toBeVisible();
  for (const control of ['share-search', 'share-open', 'share-public']) {
    await expect(dialog.getByTestId(control)).toHaveCount(0);
  }
  await expect(dialog.getByRole('combobox')).toHaveCount(0);
});
