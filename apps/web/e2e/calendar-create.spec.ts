/**
 * Making a card on a day of the calendar (question C11, answer B): "New card
 * on …" at the end of the cards of a day, wherever they are listed, and with
 * a mouse the "+" of a day's cell. The card has that day as its date and
 * opens, as on a board (C12); a view's filter gets the values it asks for,
 * as a board and a table give them. One undo takes the card away.
 *
 * The same welcome board, moment and projects as e2e/calendar.spec.ts.
 */
import { expect, test, type Page } from '@playwright/test';

import {
  calendar,
  CALENDAR_PAGE,
  day,
  dayButton,
  dueOf,
  focused,
  listOf,
  openCalendar,
  tabTo,
  wide,
} from './calendar.support.ts';
import { dialog, touch } from './kanban.support.ts';

test.use(CALENDAR_PAGE);

/** The cards of the open board with a title, as the board store has them. */
async function cardCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      (window as unknown as { fvociDev: { workspace: { board: { cards: unknown[] } } } }).fvociDev
        .workspace.board.cards.length,
  );
}

test('a card is made on an empty day with the keys alone, and one undo takes it away', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  const before = await cardCount(page);
  await expect(dayButton(page, '2026-11-25')).toHaveAccessibleName(/^Wednesday, November 25$/);
  await page.getByTestId('calendar-undated').focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowRight');
  await expect(focused(page)).toHaveAccessibleName(/^Wednesday, November 25/);
  if (wide(info)) await page.keyboard.press('Enter');
  const list = listOf(page, info);
  await expect(list).toContainText('No cards on this day.');
  await tabTo(page, list.getByRole('button', { name: 'New card on November 25' }));
  await page.keyboard.press('Enter');

  // The card opens, asking for its title; it has the day as its date.
  await expect(dialog(page)).toBeVisible();
  await expect.poll(() => cardCount(page)).toBe(before + 1);
  const made = await page.evaluate(() => {
    const store = (
      window as unknown as {
        fvociDev: {
          workspace: { board: { cards: { id: string; title: string; createAt: number }[] } };
        };
      }
    ).fvociDev.workspace.board;
    return [...store.cards].sort((a, b) => b.createAt - a.createAt)[0]?.title ?? 'none';
  });
  expect(made).toBe('');
  await expect.poll(() => dueOf(page, '')).toEqual({ from: '2026-11-25' });
  // The first Escape leaves the title, the second closes the card.
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toBeHidden();
  await expect(dayButton(page, '2026-11-25')).toHaveAccessibleName(/November 25, 1 card$/);

  // With a mouse the card gives the focus back to its new bar. Elsewhere it has none to go to:
  // the title it asked for took the focus before the card noted where it came from
  // (card/CardTitle.svelte, outside the calendar). Ctrl+Z takes the card away either way.
  if (!touch(info) && wide(info)) await expect(focused(page)).toHaveAccessibleName(/November 25/);
  await page.keyboard.press('Control+z');
  await expect.poll(() => cardCount(page)).toBe(before);
  await expect(dayButton(page, '2026-11-25')).toHaveAccessibleName(/^Wednesday, November 25$/);
});

test('with a mouse the "+" of a day makes a card there; under a finger the day is one button', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  const before = await cardCount(page);
  const cell = day(page, '2026-11-26');
  const plus = calendar(page).locator('[data-new-card="2026-11-26"]');
  if (touch(info) || !wide(info)) {
    // The cell is the button of the day (C2); cards are made from its list.
    await expect(plus).toHaveCount(0);
    return;
  }
  await expect(plus).toHaveCSS('opacity', '0');
  await cell.hover();
  await expect(plus).toHaveCSS('opacity', '1');
  await plus.click();
  await expect(dialog(page)).toBeVisible();
  await expect.poll(() => cardCount(page)).toBe(before + 1);
  await expect.poll(() => dueOf(page, '')).toEqual({ from: '2026-11-26' });
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toBeHidden();
  // Back on the card's new bar.
  await expect(focused(page)).toHaveAttribute('data-bar', /.+/);
});
