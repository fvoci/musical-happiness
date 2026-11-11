/**
 * Changing a card's date on the calendar without a drag (question C9,
 * answer B): "Change date" in the "…" of the card's line opens a month of
 * days, the card moves whole to the day picked, and one undo brings it back.
 * A calendar placed by when cards were changed offers no way and says why.
 *
 * The same welcome board, moment and projects as e2e/calendar.spec.ts.
 */
import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test';

import { calendar, CALENDAR_PAGE, dayList, openCalendar, wide } from './calendar.support.ts';

test.use(CALENDAR_PAGE);

const focused = (page: Page): Locator => page.locator(':focus');

/** The button of a day, whatever the layout draws. */
const dayButton = (page: Page, date: string): Locator =>
  calendar(page).locator(
    `[data-day="${date}"][data-day-button], [data-day="${date}"] [data-day-button]`,
  );

/** The value of "Due Date" of a card, as the board store has it. */
async function dueOf(page: Page, title: string): Promise<unknown> {
  return page.evaluate((wanted) => {
    interface Store {
      board: { cardProperties: { id: string; name: string }[] };
      cards: { title: string; fields: { properties: Record<string, unknown> } }[];
    }
    const store = (window as unknown as { fvociDev: { workspace: { board: Store } } }).fvociDev
      .workspace.board;
    const due = store.board.cardProperties.find((property) => property.name === 'Due Date');
    const card = store.cards.find((each) => each.title === wanted);
    return due && card ? (card.fields.properties[due.id] ?? null) : 'missing';
  }, title);
}

/** Presses Tab until the focus is on `target`; fails after `most` presses. */
async function tabTo(page: Page, target: Locator, most = 12): Promise<void> {
  for (let press = 0; press < most; press += 1) {
    if (await target.evaluate((element) => element === document.activeElement)) return;
    await page.keyboard.press('Tab');
  }
  await expect(target).toBeFocused();
}

/** The list a day's cards are in: the popover next to it, or the list under the small grid. */
const listOf = (page: Page, info: TestInfo): Locator =>
  wide(info) ? page.getByTestId('day-popover') : dayList(page);

/** With the keys alone: the 12th's list, the card's "…", "Change date". */
async function changeDateByKeys(page: Page, info: TestInfo, title: string): Promise<void> {
  await page.getByTestId('calendar-undated').focus();
  await page.keyboard.press('Tab');
  // The picked day takes the focus: today, the 10th, two days before the 12th, at first.
  if ((await calendar(page).getAttribute('data-selected')) === '2026-11-10') {
    await expect(focused(page)).toHaveAccessibleName(/Tuesday, November 10/);
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
  }
  await expect(focused(page)).toHaveAccessibleName(/^Thursday, November 12/);
  if (wide(info)) await page.keyboard.press('Enter');
  const list = listOf(page, info);
  await expect(list.locator('[data-card]').filter({ hasText: title })).toBeVisible();
  await tabTo(page, list.getByRole('button', { name: `Card menu: ${title}` }));
  await page.keyboard.press('Enter');
  const change = page.locator('[data-action="change-date"]');
  await expect(change).toHaveText('Change date');
  // A dropdown puts the focus on its first line; a sheet on itself.
  await tabTo(page, change);
  await page.keyboard.press('Enter');
}

test('a card moves to another day with the keys alone, and one undo brings it back', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  await expect(dayButton(page, '2026-11-12')).toHaveAccessibleName(/November 12, 2 cards/);
  await changeDateByKeys(page, info, 'Drag cards');

  const picker = page.getByTestId('day-picker');
  await expect(picker).toBeVisible();
  await expect(picker).toContainText('Now November 12.');
  // The focus is on the card's day in a popover; a sheet takes it itself, and Tab goes on to
  // the month's arrows and then the day. A week down is the 19th.
  const twelfth = picker.locator('[data-bits-day][data-value="2026-11-12"]');
  if (info.project.name === 'mobile') await tabTo(page, twelfth);
  await expect(twelfth).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');

  await expect(picker).toBeHidden();
  await expect.poll(() => dueOf(page, 'Drag cards')).toEqual({ from: '2026-11-19' });
  await expect(page.getByTestId('calendar-status')).toHaveText(
    'Drag cards is now on Thursday, November 19.',
  );
  // The focus is on the day the card is on now, which says so.
  await expect(focused(page)).toHaveAccessibleName(/^Thursday, November 19, 2 cards/);
  await expect(dayButton(page, '2026-11-12')).toHaveAccessibleName(/November 12, 1 card$/);

  await page.keyboard.press('Control+z');
  await expect.poll(() => dueOf(page, 'Drag cards')).toEqual({ from: '2026-11-12' });
  await expect(dayButton(page, '2026-11-12')).toHaveAccessibleName(/November 12, 2 cards/);
  await expect(dayButton(page, '2026-11-19')).toHaveAccessibleName(/November 19, 1 card$/);
});

test('Escape leaves the date as it was and gives the focus back to the day', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  await changeDateByKeys(page, info, 'Drag cards');
  await expect(page.getByTestId('day-picker')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('day-picker')).toBeHidden();
  expect(await dueOf(page, 'Drag cards')).toEqual({ from: '2026-11-12' });
  await expect(focused(page)).toHaveAccessibleName(/^Thursday, November 12/);

  // Enter on the day the card is on already: the picker closes, and nothing changes.
  await changeDateByKeys(page, info, 'Drag cards');
  const twelfth = page
    .getByTestId('day-picker')
    .locator('[data-bits-day][data-value="2026-11-12"]');
  if (info.project.name === 'mobile') await tabTo(page, twelfth);
  await expect(twelfth).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('day-picker')).toBeHidden();
  expect(await dueOf(page, 'Drag cards')).toEqual({ from: '2026-11-12' });
  await expect(page.getByTestId('calendar-status')).toHaveText('');
});

test('a range keeps its length and a moment its time, into the next month', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  /** Picks a day in the picker the card's "…" opened, by pressing it. */
  async function moveOn(date: string, title: string, to: string): Promise<void> {
    await dayButton(page, date).click();
    const line = listOf(page, info).locator('[data-card]').filter({ hasText: title });
    await line.hover();
    await line.getByRole('button', { name: `Card menu: ${title}` }).click();
    await page.locator('[data-action="change-date"]').click();
    const picker = page.getByTestId('day-picker');
    await picker.getByRole('button', { name: /next/i }).click();
    await picker.locator(`[data-bits-day][data-value="${to}"]`).click();
    await expect(picker).toBeHidden();
  }

  await moveOn('2026-11-12', 'Share a board', '2026-12-01');
  await expect
    .poll(() => dueOf(page, 'Share a board'))
    .toEqual({
      from: '2026-12-01',
      to: '2026-12-13',
    });
  // The calendar went to the day the card is on now.
  await expect(page.getByTestId('calendar-period')).toHaveText('December 2026');
  await expect(calendar(page)).toHaveAttribute('data-selected', '2026-12-01');

  await page.getByTestId('calendar-previous').click();
  // 08:00 on the 10th in Seoul, to 08:00 on the 2nd of December.
  await moveOn('2026-11-10', 'Create a new card', '2026-12-02');
  await expect
    .poll(() => dueOf(page, 'Create a new card'))
    .toEqual({ from: Date.parse('2026-12-01T23:00:00Z') });
});

test('a calendar placed by when cards were changed offers no change, and says why', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  const header = page.getByTestId('view-header');
  if ((await header.getAttribute('data-layout')) === 'row') {
    await header.locator('[data-tool="display-by"]').click();
  } else {
    await header.locator('[data-tool="more"]').click();
    await page.locator('[role="menuitem"][data-action="display-by"]').click();
  }
  await page.getByRole('menuitem', { name: 'Last updated time' }).click();
  await dayButton(page, '2026-11-10').click();
  const list = listOf(page, info);
  await expect(list.getByTestId('dates-fixed')).toHaveText(
    'The cards are placed by Last updated time, which cannot be changed here.',
  );
  const menu = list.getByRole('button', { name: /^Card menu: / }).first();
  // What a screen reader hears on the "…" says why, as the list does.
  await expect(menu).toHaveAccessibleDescription(
    'The cards are placed by Last updated time, which cannot be changed here.',
  );
  await list.locator('[data-card]').first().hover();
  await menu.click();
  await expect(page.locator('[data-action="duplicate"]')).toBeVisible();
  await expect(page.locator('[data-action="change-date"]')).toHaveCount(0);
});
