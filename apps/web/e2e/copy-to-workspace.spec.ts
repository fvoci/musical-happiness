/**
 * Copying a board into another workspace, on the in-memory backend: the
 * reader's own space is the one place to go, so it is chosen already, and one
 * press makes a private copy under the same title, which opens. An
 * organization that lets nothing be copied out of it (`?copyOut=0`) refuses,
 * and from then on the dialog says so before anything is pressed.
 */
import { expect, test } from '@playwright/test';

import { ON_A_VIEW, placeOf, WORKSPACE } from './address.ts';
import { boardMenu, chooseFromMenu, FIRST_BOARD, openApp, showSidebar } from './shell.support.ts';

test('a board is copied into the one place there is with one press, under its title', async ({
  page,
}) => {
  await openApp(page);
  await showSidebar(page);
  await chooseFromMenu(page, boardMenu(page, FIRST_BOARD), 'copy-to-workspace');
  const dialog = page.getByTestId('copy-to-workspace-dialog');
  await expect(dialog.getByTestId('copy-to-workspace-only')).toHaveText('Copies into My space');
  await expect(dialog.getByRole('textbox')).toHaveCount(0);
  await expect(dialog.getByTestId('copy-to-workspace-target')).toHaveCount(0);
  await dialog.getByTestId('copy-to-workspace-submit').click();
  await expect(dialog).toBeHidden();
  await expect(page).toHaveURL(ON_A_VIEW);
  expect(placeOf(page.url()).workspaceId).not.toBe(WORKSPACE);
  await expect(page.getByTestId('top-bar').getByTestId('board-title')).toHaveValue(FIRST_BOARD);
});

test('an organization that keeps its boards in says so, once it has refused', async ({ page }) => {
  await openApp(page, '/?copyOut=0');
  await showSidebar(page);
  await chooseFromMenu(page, boardMenu(page, FIRST_BOARD), 'copy-to-workspace');
  const dialog = page.getByTestId('copy-to-workspace-dialog');
  await expect(dialog.getByTestId('copy-to-workspace-kept')).toHaveCount(0);
  await dialog.getByTestId('copy-to-workspace-submit').click();
  await expect(dialog.getByTestId('copy-to-workspace-error')).toContainText(
    'does not allow copying',
  );
  await expect(dialog.getByTestId('copy-to-workspace-kept')).toBeVisible();
  await expect(dialog.getByTestId('copy-to-workspace-submit')).toBeDisabled();
  const target = dialog.getByTestId('copy-to-workspace-target');
  await expect(target.getByRole('option')).toHaveText([
    'My space — outside the organization, cannot copy',
  ]);
  await expect(target.getByRole('option')).toBeDisabled();
});
