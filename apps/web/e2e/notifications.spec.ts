/**
 * The bell of the top bar, end to end, on the in-memory backend: a comment
 * somebody else writes on a card the reader wrote shows on the bell at once,
 * without a reload; the notification opens the card at that comment and is
 * read by it; and the switch of comments, turned off in the settings, is
 * kept, and nothing is told while it is off. Runs at the three stages.
 *
 * Somebody else is the other person of the same world (`fvociDev.someoneElse`).
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import { openApp, showSidebar, sidebar, strip } from './shell.support.ts';

interface DevBackend {
  createCard(boardId: string, draft: { title: string }): Promise<{ id: string }>;
  createComment(boardId: string, cardId: string, body: unknown): Promise<{ id: string }>;
  settings(): Promise<Record<string, unknown>>;
}

interface Dev {
  backend: DevBackend;
  someoneElse(): DevBackend;
}

interface Place {
  boardId: string;
  cardId: string;
}

const bell = (page: Page): Locator => page.getByTestId('open-notifications');
const badge = (page: Page): Locator => page.getByTestId('unread-notifications');
const inbox = (page: Page): Locator => page.getByTestId('notifications');

/** A card the reader writes on the board shown, which the other person is a member of. */
async function cardOfMine(page: Page, title: string): Promise<Place> {
  const boardId = /\/boards\/([^/]+)\//.exec(new URL(page.url()).pathname)?.[1] ?? '';
  const cardId = await page.evaluate(
    async ({ boardId, title }) => {
      const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
      return (await dev.backend.createCard(boardId, { title })).id;
    },
    { boardId, title },
  );
  return { boardId, cardId };
}

/** The other person comments on the card: the comment's identifier. */
function theyComment(page: Page, place: Place, words: string): Promise<string> {
  return page.evaluate(
    async ({ boardId, cardId, words }) => {
      const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
      const body = {
        type: 'doc',
        content: [{ type: 'paragraph', content: [{ type: 'text', text: words }] }],
      };
      return (await dev.someoneElse().createComment(boardId, cardId, body)).id;
    },
    { ...place, words },
  );
}

/** Opens the settings from the reader's account: on the strip where there is one, else in the sidebar. */
async function openSettings(page: Page): Promise<Locator> {
  if (await strip(page).isVisible()) {
    await strip(page).getByTestId('strip-open-settings').click();
  } else {
    await showSidebar(page);
    await sidebar(page).getByTestId('open-settings').click();
  }
  const dialog = page.getByTestId('settings-dialog');
  await expect(dialog).toBeVisible();
  return dialog;
}

test('a comment on the reader’s card rings the bell at once, and opens the card at the comment', async ({
  page,
}) => {
  await openApp(page);
  const place = await cardOfMine(page, 'Watched plan');
  await expect(bell(page)).toBeVisible();
  await expect(badge(page)).toBeHidden();

  const commentId = await theyComment(page, place, 'Looks good to me');
  await expect(badge(page)).toHaveText('1');
  await expect(bell(page)).toHaveAccessibleName('Notifications, 1 unread');

  await bell(page).click();
  const line = inbox(page).getByRole('link', { name: /commented on Watched plan/ });
  await expect(line).toBeVisible();
  await expect(inbox(page).locator('[data-unread]')).toHaveCount(1);
  await line.click();

  await expect(inbox(page)).toBeHidden();
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('card-title')).toHaveValue('Watched plan');
  await expect(page.locator(`[data-comment="${commentId}"]`)).toBeInViewport();
  await expect(badge(page)).toBeHidden();
  // The card closed, the bell lists it read.
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('card-dialog')).toBeHidden();
  await bell(page).click();
  await expect(inbox(page).locator('[data-notification]')).toHaveCount(1);
  await expect(inbox(page).locator('[data-unread]')).toHaveCount(0);
});

test('the switch of comments is kept off, and nothing is told while it is', async ({ page }) => {
  await openApp(page);
  const dialog = await openSettings(page);
  const comments = dialog.locator('[data-setting="notifyComments"]').getByRole('switch');
  await expect(comments).toBeChecked();
  await comments.click();
  await expect(comments).not.toBeChecked();
  await expect
    .poll(() =>
      page.evaluate(() => (window as unknown as { fvociDev: Dev }).fvociDev.backend.settings()),
    )
    .toMatchObject({ notifyComments: false, notifyMentions: true, notifyAssignments: true });

  // Kept: the page loads again with it off.
  await openApp(page);
  const again = await openSettings(page);
  const kept = again.locator('[data-setting="notifyComments"]').getByRole('switch');
  await expect(kept).not.toBeChecked();
  await page.keyboard.press('Escape');
  await expect(again).toBeHidden();

  const place = await cardOfMine(page, 'Quiet plan');
  await theyComment(page, place, 'Not told');
  // Turned on again: only what comes after is told.
  const last = await openSettings(page);
  await last.locator('[data-setting="notifyComments"]').getByRole('switch').click();
  await page.keyboard.press('Escape');
  await theyComment(page, place, 'Told');
  await expect(badge(page)).toHaveText('1');
  await bell(page).click();
  await expect(inbox(page).getByRole('link', { name: /commented on Quiet plan/ })).toBeVisible();
  await expect(inbox(page).getByRole('link', { name: /2 times/ })).toHaveCount(0);
});
