/**
 * The calendar view on the in-memory backend: making one, cards on their
 * days and bars across several, the small grid of a narrow box, the cards of
 * a day, moving through the months, and the date the view is placed by.
 *
 * The clock of the page is fixed at 2026-11-10 09:00 in Seoul, and so is its
 * time zone; the language is English, whose weeks start on Sunday. November
 * 2026 then has five weeks, from 1 November to 5 December.
 *
 * The projects are the three widths of docs/calendar-proposal.md: the
 * desktop (a pane of 1024 pixels, a mouse) and the unfolded device (570, a
 * finger) have room for bars; the phone (412) gets the small grid.
 */
import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test';

import { boardPage, dialog, fitPane, openView, touch } from './kanban.support.ts';
import { screensDir } from './screens.ts';

const WELCOME = ['Welcome to fvoci!', 'Preview: Calendar View'] as const;
const NOW = new Date('2026-11-10T00:00:00Z'); // 09:00 in Seoul

test.use({ timezoneId: 'Asia/Seoul', locale: 'en-US' });

const calendar = (page: Page): Locator => page.getByTestId('calendar');
const grid = (page: Page): Locator => page.getByTestId('month-grid');
const mini = (page: Page): Locator => page.getByTestId('mini-month');
const day = (page: Page, date: string): Locator => calendar(page).locator(`[data-day="${date}"]`);
const bars = (page: Page, title: string): Locator =>
  grid(page).locator('[data-bar]').filter({ hasText: title });
const dayList = (page: Page): Locator => page.getByTestId('day-list');
const wide = (info: TestInfo): boolean => info.project.name !== 'mobile';

/** A date value as the wire has it: a day as text, a moment as milliseconds. */
type Stored = { from: string | number; to?: string | number } | undefined;

/** Gives cards of the open board a value of "Due Date", through the board store. */
async function setDates(page: Page, values: Record<string, Stored>): Promise<void> {
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
async function openCalendar(page: Page, info: TestInfo): Promise<void> {
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

test('a calendar view is made from the list of kinds, placed by the first date', async ({
  page,
}, info) => {
  await page.clock.setFixedTime(NOW);
  await openView(page, info, 'Welcome to fvoci!', 'Onboarding');
  const header = page.getByTestId('view-header');
  if ((await header.getAttribute('data-layout')) === 'row') {
    await header.locator('[data-tool="add-view"]').click();
    await page.getByRole('menuitem', { name: 'Calendar', exact: true }).click();
  } else {
    await header.locator('[data-tool="views"]').click();
    await page.getByRole('menuitem', { name: 'Add calendar view' }).click();
  }
  await expect(calendar(page)).toBeVisible();
  await expect(page.getByTestId('calendar-period')).toHaveText('November 2026');
  if ((await header.getAttribute('data-layout')) === 'row') {
    await expect(header.locator('[data-tool="display-by"]')).toHaveAccessibleName(
      'Display by: Due Date',
    );
  } else {
    await header.locator('[data-tool="more"]').click();
    await expect(page.locator('[role="menuitem"][data-action="display-by"]')).toContainText(
      'Due Date',
    );
  }
});

test('the weeks of the month start on Sunday, and today is marked', async ({ page }, info) => {
  await openCalendar(page, info);
  const weeks = calendar(page).locator('[data-week]');
  await expect(weeks).toHaveCount(5);
  await expect(weeks.first()).toHaveAttribute('data-week', '2026-11-01');
  await expect(calendar(page).locator('[data-weekday]').first()).toHaveText('Sun');
  const today = calendar(page).locator('[aria-current="date"]');
  await expect(today).toHaveCount(1);
  await expect(today).toHaveAccessibleName(/Tuesday, November 10/);
});

test('a dated card is on its day, a range is a bar in each week it crosses', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  // The phone is given room for bars: the grid is the same under a finger.
  if (!wide(info)) await fitPane(page, 700);
  // "Drag cards" is on Thursday the 12th: the fifth column of the second week.
  const week = (date: string): Locator => grid(page).locator(`[data-week="${date}"]`);
  await expect(
    week('2026-11-08').locator('[data-bar]').filter({ hasText: 'Drag cards' }),
  ).toHaveCount(1);
  // The range is three bars, cut square where it goes on.
  await expect(bars(page, 'Share a board')).toHaveCount(3);
  const [first, second, third] = await bars(page, 'Share a board').all();
  const grid8 = await week('2026-11-08').boundingBox();
  const box1 = await first?.boundingBox();
  const box2 = await second?.boundingBox();
  const box3 = await third?.boundingBox();
  if (!grid8 || !box1 || !box2 || !box3) throw new Error('the bars are not on the page');
  const column = grid8.width / 7;
  // Thursday to Saturday, the whole week, Sunday to Tuesday.
  expect(Math.round(box1.width / column)).toBe(3);
  expect(Math.round(box2.width / column)).toBe(7);
  expect(Math.round(box3.width / column)).toBe(3);
  expect(box1.x + box1.width).toBeCloseTo(grid8.x + grid8.width, -1);
  // A moment is on the reader's day, with its time.
  await expect(
    week('2026-11-08').locator('[data-bar]').filter({ hasText: 'Create a new card' }),
  ).toContainText('8:00');
  // A day says how many cards it has.
  await expect(
    calendar(page).getByRole('button', { name: 'Thursday, November 12, 2 cards' }),
  ).toHaveCount(1);
});

test('a day with more cards than rows says "+N", and the day lists them all', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  if (!wide(info)) await fitPane(page, 700);
  const crowd = ['Manage tasks with cards', 'Create your own board', 'Share cards on Channels'];
  await setDates(page, Object.fromEntries(crowd.map((title) => [title, { from: '2026-11-12' }])));
  const more = grid(page).locator('[data-more="2026-11-12"]');
  await expect(more).toBeVisible();
  const lanes = Number(await grid(page).getAttribute('data-lanes'));
  const hidden = Number((await more.textContent())?.replace('+', ''));
  // Five cards: every row but the last has a bar, the last says how many more.
  expect(hidden).toBe(5 - (lanes - 1));

  if (touch(info)) await day(page, '2026-11-12').click();
  else await more.click();
  const list = page.getByTestId('day-popover');
  await expect(list).toBeVisible();
  await expect(list.locator('[data-card]')).toHaveCount(5);
  await expect(list).toContainText('Thursday, November 12');
  // A card of several days says which of them this is.
  await expect(list.locator('[data-card]').filter({ hasText: 'Share a board' })).toContainText(
    'Day 1 of 13',
  );
  await page.keyboard.press('Escape');
  await expect(list).toBeHidden();
});

test('a card opens from its bar with a mouse, from the list of its day with a finger', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  if (wide(info) && !touch(info)) {
    await bars(page, 'Drag cards').click();
  } else {
    await day(page, '2026-11-12').click();
    const list = wide(info) ? page.getByTestId('day-popover') : dayList(page);
    await list.locator('[data-card]').filter({ hasText: 'Drag cards' }).locator('a').click();
  }
  await expect(dialog(page)).toContainText('Drag cards');
  await expect(page).toHaveURL(/\/board\/[^/]+\/[^/]+\/[^/]+$/);
});

test('a narrow box has the small grid over the list of the picked day', async ({ page }, info) => {
  await openCalendar(page, info);
  // The width of the phone, in every project: a mouse gets the same small grid.
  if (wide(info)) await fitPane(page, 412);
  await expect(mini(page)).toBeVisible();
  // Today is picked first; its list has the card of 08:00.
  await expect(dayList(page)).toHaveAttribute('data-date', '2026-11-10');
  await expect(dayList(page).locator('[data-card]')).toHaveCount(1);
  await expect(dayList(page)).toContainText('8:00');
  // The range is a line across its days, the card of one day a dot.
  await expect(mini(page).locator('[data-line]')).toHaveCount(3);
  await day(page, '2026-11-12').click();
  await expect(dayList(page)).toHaveAttribute('data-date', '2026-11-12');
  await expect(dayList(page).locator('[data-card]')).toHaveCount(2);
  // A day is a button wide enough for a thumb.
  const box = await day(page, '2026-11-12').boundingBox();
  expect(box?.width).toBeGreaterThanOrEqual(44);
  expect(box?.height).toBeGreaterThanOrEqual(44);
});

test('the grid turns into the small one at 34rem of box, and keeps the month', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  await page.getByTestId('calendar-next').click();
  await expect(page.getByTestId('calendar-period')).toHaveText('December 2026');
  await fitPane(page, 543);
  await expect(calendar(page)).toHaveAttribute('data-layout', 'mini');
  await expect(page.getByTestId('calendar-period')).toHaveText('December 2026');
  await fitPane(page, 544);
  await expect(calendar(page)).toHaveAttribute('data-layout', 'bars');
  await expect(calendar(page)).toHaveAttribute('data-month', '2026-12');
  // Nothing is wider than the page.
  const overflow = await boardPage(page).evaluate(
    (element) => element.scrollWidth - element.clientWidth,
  );
  expect(overflow).toBe(0);
});

test('the arrows go a month on and back, and "Today" comes back', async ({ page }, info) => {
  await openCalendar(page, info);
  await page.getByTestId('calendar-next').click();
  await expect(page.getByTestId('calendar-period')).toHaveText('December 2026');
  await expect(calendar(page).locator('[data-week]').first()).toHaveAttribute(
    'data-week',
    '2026-11-29',
  );
  await page.getByTestId('calendar-previous').click();
  await page.getByTestId('calendar-previous').click();
  await expect(page.getByTestId('calendar-period')).toHaveText('October 2026');
  await page.getByTestId('calendar-today').click();
  await expect(page.getByTestId('calendar-period')).toHaveText('November 2026');
  await expect(calendar(page)).toHaveAttribute('data-selected', '2026-11-10');
});

test('the days are one stop of Tab, and the arrows walk them into the next month', async ({
  page,
}, info) => {
  await openCalendar(page, info);
  const picked = calendar(page).locator('[data-day-button][tabindex="0"]');
  await expect(picked).toHaveCount(1);
  await picked.focus();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowRight');
  await expect(calendar(page)).toHaveAttribute('data-selected', '2026-11-18');
  await expect(page.locator(':focus')).toHaveAccessibleName(/November 18/);
  for (let step = 0; step < 3; step += 1) await page.keyboard.press('ArrowDown');
  await expect(calendar(page)).toHaveAttribute('data-selected', '2026-12-09');
  await expect(page.getByTestId('calendar-period')).toHaveText('December 2026');
  if (wide(info)) {
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('day-popover')).toBeVisible();
    await page.keyboard.press('Escape');
  }
  await expect(page.locator(':focus')).toHaveAccessibleName(/December 9/);
  await expect(calendar(page).locator('[data-day-button][tabindex="0"]')).toHaveCount(1);
});

test('"display by" places the cards by another date', async ({ page }, info) => {
  await openCalendar(page, info);
  await expect(day(page, '2026-11-12').first()).toBeVisible();
  const header = page.getByTestId('view-header');
  if ((await header.getAttribute('data-layout')) === 'row') {
    await header.locator('[data-tool="display-by"]').click();
  } else {
    await header.locator('[data-tool="more"]').click();
    await page.locator('[role="menuitem"][data-action="display-by"]').click();
  }
  await page.getByRole('menuitem', { name: 'Last updated time' }).click();
  // Every card was last changed on the seed's day, a month ago, or today by this test.
  await expect(
    calendar(page).getByRole('button', { name: /^Thursday, November 12, \d+ cards?$/ }),
  ).toHaveCount(0);
  await expect(
    calendar(page).getByRole('button', { name: /^Tuesday, November 10, 4 cards$/ }),
  ).toHaveCount(1);
});

/**
 * Pictures for a person to look at, in `target/screens/calendar/` when
 * `FVOCI_SCREENS` is set. The test runs either way: what it checks on the
 * way (the small grid at 390 pixels and nothing wider than the page, the
 * cards of a day) holds with or without pictures.
 */
test('the calendar in light and dark, at 390 pixels, and the cards of a day', async ({
  page,
}, info) => {
  const STAGE: Record<string, string> = {
    mobile: 'compact',
    middle: 'medium',
    desktop: 'expanded',
  };
  const stage = STAGE[info.project.name] ?? 'expanded';
  const shoot = async (name: string): Promise<void> => {
    if (process.env.FVOCI_SCREENS) {
      await page.screenshot({ path: `${screensDir('calendar')}/calendar-${name}.png` });
    }
  };
  await openCalendar(page, info);
  await setDates(page, {
    'Manage tasks with cards': { from: '2026-11-12' },
    'Create your own board': { from: '2026-11-12' },
    'Share cards on Channels': { from: '2026-11-03', to: '2026-11-05' },
    'Create a new view': { from: '2026-11-30', to: '2026-12-02' },
  });
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    if (scheme === 'dark') await expect(page.locator('html')).toHaveClass(/\bdark\b/);
    else await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
    await shoot(`${stage}-${scheme}`);
  }
  await page.emulateMedia({ colorScheme: 'light' });

  if (!wide(info)) await fitPane(page, 700);
  if (touch(info)) await day(page, '2026-11-12').click();
  else await day(page, '2026-11-12').locator('[data-day-button]').click();
  await expect(page.getByTestId('day-popover').locator('[data-card]')).toHaveCount(4);
  await shoot(`day-${stage}`);
  await page.keyboard.press('Escape');

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(calendar(page)).toHaveAttribute('data-layout', 'mini');
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBe(0);
  await shoot(`390-${info.project.name}`);
});
