/**
 * The app on its in-memory backend, end to end: the address bar and the
 * screens stay in step. The parts of the page have tests of their own.
 */
import { expect, test } from '@playwright/test';

test('the root shows the first board of the sidebar, and its first view', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  await expect(page).toHaveURL(/\/board\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
});

test('a card is an address: opening adds a step, back closes it, forward opens it again', async ({
  page,
}) => {
  await page.goto('/');
  const card = page.locator('[data-card]').first();
  await expect(card).toBeVisible();
  const board = page.url();
  await card.click();
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page).toHaveURL(/\/board\/[0-9a-f-]{36}\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  const opened = page.url();

  await page.goBack();
  await expect(page.getByTestId('card-dialog')).toBeHidden();
  expect(page.url()).toBe(board);

  await page.goForward();
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  expect(page.url()).toBe(opened);

  // Loaded on its own address, the card is open over its board.
  await page.goto(opened);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('board-page')).toBeVisible();
});
