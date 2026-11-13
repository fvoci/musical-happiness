/**
 * What the tests of the calendar share: the welcome board's calendar with a
 * few dated cards, at a fixed moment (2026-11-10 01:00 in Seoul, still the
 * 9th in UTC), and the parts of the page they look for. A spec that uses it
 * fixes the page's time zone and language with `CALENDAR_PAGE`.
 */
import { expect, type Locator, type Page, type TestInfo } from '@playwright/test';

import { openView } from './kanban.support.ts';
import { tabKey } from './keyboard.support.ts';

/** The page's time zone and language: Seoul, and English, whose weeks start on Sunday. */
export const CALENDAR_PAGE = { timezoneId: 'Asia/Seoul', locale: 'en-US' } as const;

export const WELCOME = ['Welcome to fvoci!', 'Preview: Calendar View'] as const;
export const NOW = new Date('2026-11-09T16:00:00Z'); // 01:00 on the 10th in Seoul

export const calendar = (page: Page): Locator => page.getByTestId('calendar');
export const grid = (page: Page): Locator => page.getByTestId('month-grid');
export const mini = (page: Page): Locator => page.getByTestId('mini-month');
export const day = (page: Page, date: string): Locator =>
  calendar(page).locator(`[data-day="${date}"]`);
export const bars = (page: Page, title: string): Locator =>
  grid(page).locator('[data-bar]').filter({ hasText: title });
export const dayList = (page: Page): Locator => page.getByTestId('day-list');
export const wide = (info: TestInfo): boolean => info.project.name !== 'mobile';

/** A date value as the wire has it: a day as text, a moment as milliseconds. */
export type Stored = { from: string | number; to?: string | number } | undefined;

/** Gives cards of the open board a value of "Due Date", through the board store. */
export async function setDates(page: Page, values: Record<string, Stored>): Promise<void> {
  await page.evaluate(async (wanted) => {
    interface Store {
      board: { cardProperties: { id: string; name: string }[] };
      cards: { id: string; title: string }[];
      setCardProperty(cardId: string, propertyId: string, value: unknown): Promise<void>;
    }
    const { workspace } = (window as unknown as { fvociDev: { workspace: { board: Store } } })
      .fvociDev;
    const store = workspace.board;
    const due = store.board.cardProperties.find((property) => property.name === 'Due Date');
    for (const [title, value] of Object.entries(wanted)) {
      const card = store.cards.find((each) => each.title === title);
      if (!card || !due) throw new Error(`no card "${title}" or no Due Date`);
      await store.setCardProperty(card.id, due.id, value);
    }
  }, values);
}

/** Opens the calendar of the welcome board with a few dated cards. */
export async function openCalendar(page: Page, info: TestInfo): Promise<void> {
  await page.clock.setFixedTime(NOW);
  await openView(page, info, ...WELCOME);
  await setDates(page, {
    'Drag cards': { from: '2026-11-12' },
    // Three weeks: 12 to 24 November.
    'Share a board': { from: '2026-11-12', to: '2026-11-24' },
    // 08:00 on the 10th in Seoul.
    'Create a new card': { from: Date.parse('2026-11-09T23:00:00Z') },
    'Filter and sort cards': { from: '2026-12-03' },
  });
  await expect(calendar(page)).toHaveAttribute('data-layout', wide(info) ? 'bars' : 'mini');
}

export const focused = (page: Page): Locator => page.locator(':focus');

/** The button of a day, whatever the layout draws. */
export const dayButton = (page: Page, date: string): Locator =>
  calendar(page).locator(
    `[data-day="${date}"][data-day-button], [data-day="${date}"] [data-day-button]`,
  );

/** The value of "Due Date" of a card, as the board store has it. */
export async function dueOf(page: Page, title: string): Promise<unknown> {
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
export async function tabTo(page: Page, target: Locator, most = 12): Promise<void> {
  for (let press = 0; press < most; press += 1) {
    if (await target.evaluate((element) => element === document.activeElement)) return;
    await page.keyboard.press(await tabKey(page));
  }
  await expect(target).toBeFocused();
}

/** The list a day's cards are in: the popover next to it, or the list under the small grid. */
export const listOf = (page: Page, info: TestInfo): Locator =>
  wide(info) ? page.getByTestId('day-popover') : dayList(page);
