/**
 * The Gantt view on the in-memory backend: making one, cards as bars on
 * their days, the line of today, the keys that walk the bars, and a narrow
 * page.
 *
 * The clock of the page is fixed at 2026-11-10 01:00 in Seoul (still the
 * 9th in UTC, so that a day taken from UTC would show), and so is its time
 * zone; the language is English, whose weeks start on Sunday.
 */
import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test';

import { dialog, openView } from './kanban.support.ts';
import { screensDir } from './screens.ts';

const WELCOME = ['Welcome to fvoci!', 'Onboarding'] as const;
const NOW = new Date('2026-11-09T16:00:00Z'); // 01:00 on the 10th in Seoul

test.use({ timezoneId: 'Asia/Seoul', locale: 'en-US' });

const gantt = (page: Page): Locator => page.getByTestId('gantt');
const scroller = (page: Page): Locator => page.getByTestId('gantt-scroller');
const bars = (page: Page): Locator => gantt(page).locator('[data-bar]');
const bar = (page: Page, title: string): Locator =>
  gantt(page)
    .locator('[data-row]')
    .filter({ has: page.locator('[data-row-title]', { hasText: title }) })
    .locator('[data-bar]');

/** A date value as the wire has it: a day as text, a moment as milliseconds. */
type Stored = { from: string | number; to?: string | number } | undefined;

interface Store {
  boardId: string;
  board: { cardProperties: { id: string; name: string; type: string }[] };
  cards: { id: string; title: string }[];
  views: { id: string; title: string; fields: Record<string, unknown> }[];
  setCardProperty(cardId: string, propertyId: string, value: unknown): Promise<void>;
}
interface Dev {
  workspace: { board: Store };
  backend: {
    views(boardId: string): Promise<{ id: string; fields: Record<string, unknown> }[]>;
  };
}

/** Gives cards of the open board a value of "Due Date", through the board store. */
async function setDates(page: Page, values: Record<string, Stored>): Promise<void> {
  await page.evaluate(async (wanted) => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    const due = store.board.cardProperties.find((property) => property.name === 'Due Date');
    for (const [title, value] of Object.entries(wanted)) {
      const card = store.cards.find((each) => each.title === title);
      if (!card || !due) throw new Error(`no card "${title}" or no Due Date`);
      await store.setCardProperty(card.id, due.id, value);
    }
  }, values);
}

/** The fields of the view that is open, as the backend keeps them. */
async function storedFields(page: Page): Promise<Record<string, unknown>> {
  const viewId = /\/board\/[^/]+\/([^/]+)/.exec(new URL(page.url()).pathname)?.[1] ?? '';
  return page.evaluate(async (id) => {
    const { workspace, backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const views = await backend.views(workspace.board.boardId);
    return views.find((view) => view.id === id)?.fields ?? {};
  }, viewId);
}

/** Adds a Gantt view to the board that is open, from the list of kinds. */
async function addGantt(page: Page): Promise<void> {
  const header = page.getByTestId('view-header');
  if ((await header.getAttribute('data-layout')) === 'row') {
    await header.locator('[data-tool="add-view"]').click();
    await page.getByRole('menuitem', { name: 'Gantt', exact: true }).click();
  } else {
    await header.locator('[data-tool="views"]').click();
    await page.getByRole('menuitem', { name: 'Add Gantt view' }).click();
  }
  await expect(gantt(page)).toBeVisible();
  await expect(scroller(page)).toBeVisible();
}

/** A Gantt view of the welcome board with a few dated cards. */
async function openGantt(page: Page, info: TestInfo): Promise<void> {
  await page.clock.setFixedTime(NOW);
  await openView(page, info, ...WELCOME);
  await setDates(page, {
    'Drag cards': { from: '2026-11-12' },
    // Twelve days: 12 to 23 November.
    'Share a board': { from: '2026-11-12', to: '2026-11-23' },
    // 08:00 on the 10th in Seoul.
    'Create a new card': { from: Date.parse('2026-11-09T23:00:00Z') },
    'Filter and sort cards': { from: '2026-12-03', to: '2026-12-04' },
  });
  await addGantt(page);
  await expect(bars(page)).toHaveCount(4);
}

/** Where a day begins across the page, and how wide a day is: read off the marks of the axis. */
async function dayScale(page: Page): Promise<{ x: (date: string) => number; width: number }> {
  const marks = await gantt(page)
    .locator('[data-tick]')
    .evaluateAll((ticks) =>
      ticks.slice(0, 2).map((tick) => ({
        date: tick.getAttribute('data-tick') ?? '',
        x: tick.getBoundingClientRect().x,
      })),
    );
  const [first, second] = marks;
  if (!first || !second) throw new Error('the axis has no marks');
  const days = (from: string, to: string): number =>
    Math.round((Date.parse(to) - Date.parse(from)) / 86_400_000);
  const width = (second.x - first.x) / days(first.date, second.date);
  return { x: (date) => first.x + days(first.date, date) * width, width };
}

test('a Gantt view is made from the list of kinds, placed by the first date, zoomed to weeks', async ({
  page,
}, info) => {
  await page.clock.setFixedTime(NOW);
  await openView(page, info, ...WELCOME);
  await addGantt(page);
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'week');
  const header = page.getByTestId('view-header');
  if ((await header.getAttribute('data-layout')) === 'row') {
    await expect(header.locator('[data-tool="display-by"]')).toHaveAccessibleName(
      'Display by: Due Date',
    );
  } else {
    await header.locator('[data-tool="more"]').click();
    await expect(page.locator('[role="menuitem"][data-action="display-by"]')).toContainText(
      'Due Date',
    );
    await page.keyboard.press('Escape');
  }
  const fields = await storedFields(page);
  expect(fields.viewType).toBe('gantt');
  expect(fields.ganttZoom).toBe('week');
  const due = await page.evaluate(
    () =>
      (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board.board.cardProperties.find(
        (property) => property.name === 'Due Date',
      )?.id,
  );
  expect(fields.dateDisplayPropertyId).toBe(due);
  // No card of the board has a date yet.
  await expect(page.getByTestId('gantt-empty')).toBeVisible();
  // The axis has today on it and begins on a Sunday.
  await expect(gantt(page)).toHaveAttribute('data-first', '2026-10-25');
});

test('a dated card is a bar from its first day to its last, a card without a date has none', async ({
  page,
}, info) => {
  await openGantt(page, info);
  const scale = await dayScale(page);
  /** A bar lies on these days: `days` of them from `first`. */
  async function lies(title: string, first: string, days: number): Promise<void> {
    await bar(page, title).scrollIntoViewIfNeeded();
    const now = await dayScale(page);
    const box = await bar(page, title).boundingBox();
    if (!box) throw new Error(`"${title}" has no bar`);
    expect(Math.abs(box.x - now.x(first))).toBeLessThan(1);
    expect(Math.abs(box.width - days * now.width)).toBeLessThan(1);
  }
  expect(scale.width).toBeGreaterThan(4);
  await lies('Drag cards', '2026-11-12', 1);
  await lies('Share a board', '2026-11-12', 12);
  // A moment is on the reader's day: the 10th in Seoul, not the 9th of UTC.
  await lies('Create a new card', '2026-11-10', 1);
  // Into the next month.
  await lies('Filter and sort cards', '2026-12-03', 2);
  await expect(bar(page, 'Share a board')).toHaveAccessibleName('Share a board, November 12 – 23');
  await expect(bar(page, 'Create a new card')).toHaveAccessibleName(
    '8:00 AM Create a new card, November 10',
  );
  // Every row is a card with a bar.
  await expect(gantt(page).locator('[data-row]')).toHaveCount(4);
});

test('today has a line through the chart, in sight when the view opens', async ({ page }, info) => {
  await openGantt(page, info);
  const line = page.getByTestId('gantt-today-line');
  await expect(line).toHaveAttribute('data-day', '2026-11-10');
  await expect(line).toBeInViewport();
  const scale = await dayScale(page);
  const box = await line.boundingBox();
  if (!box) throw new Error('no line of today');
  // In the middle of the 10th.
  expect(Math.abs(box.x + box.width / 2 - (scale.x('2026-11-10') + scale.width / 2))).toBeLessThan(
    1.5,
  );
  // "Today" brings it back after the days were scrolled away: a card next spring makes room to.
  await setDates(page, { 'Create a new view': { from: '2027-04-01' } });
  await expect(bars(page)).toHaveCount(5);
  await scroller(page).evaluate((element) => {
    element.scrollLeft = element.scrollWidth;
  });
  await expect(line).not.toBeInViewport();
  await page.getByTestId('gantt-today').click();
  await expect(line).toBeInViewport();
});

test('the titles stay at the left and the marks at the top while the chart scrolls', async ({
  page,
}, info) => {
  await openGantt(page, info);
  const title = gantt(page).locator('[data-row-title]').first();
  const axis = page.getByTestId('gantt-axis');
  const before = { title: await title.boundingBox(), axis: await axis.boundingBox() };
  await scroller(page).evaluate((element) => {
    element.scrollLeft += 300;
  });
  await expect
    .poll(async () => (await scroller(page).evaluate((element) => element.scrollLeft)) > 0)
    .toBe(true);
  const after = { title: await title.boundingBox(), axis: await axis.boundingBox() };
  expect(after.title?.x).toBe(before.title?.x);
  expect(after.axis?.y).toBe(before.axis?.y);
  // The column of titles leaves most of a narrow page to the days.
  const box = await scroller(page).boundingBox();
  expect((after.title?.width ?? 0) / (box?.width ?? 1)).toBeLessThan(0.4);
});

test('the bars are one stop of Tab, the arrows walk them, and a bar opens its card', async ({
  page,
}, info) => {
  await openGantt(page, info);
  const stops = gantt(page).locator('[data-bar][tabindex="0"]');
  await expect(stops).toHaveCount(1);
  await stops.focus();
  const first = await stops.getAttribute('data-bar');
  await page.keyboard.press('ArrowDown');
  await expect(stops).toHaveCount(1);
  await expect(stops).toBeFocused();
  expect(await stops.getAttribute('data-bar')).not.toBe(first);
  await page.keyboard.press('End');
  await expect(bars(page).last()).toBeFocused();
  await page.keyboard.press('Home');
  await expect(bars(page).first()).toBeFocused();
  const title = await gantt(page).locator('[data-row-title]').first().textContent();
  await page.keyboard.press('Enter');
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page).locator('[data-card-title]')).toHaveValue((title ?? '').trim());
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toHaveCount(0);
  // The focus comes back to the bar the card was opened from.
  await expect(bars(page).first()).toBeFocused();
});

test('the title of a row opens its card too', async ({ page }, info) => {
  await openGantt(page, info);
  await gantt(page).locator('[data-row-title]', { hasText: 'Share a board' }).click();
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page).locator('[data-card-title]')).toHaveValue('Share a board');
});

/**
 * Pictures for a person to look at, in `target/screens/gantt/` when
 * `FVOCI_SCREENS` is set. The test runs either way: what it checks on the
 * way (nothing wider than the page at 390 pixels) holds with or without
 * pictures.
 */
test('the chart in light and dark, and at 390 pixels', async ({ page }, info) => {
  const shoot = async (name: string): Promise<void> => {
    if (process.env.FVOCI_SCREENS) {
      await page.screenshot({ path: `${screensDir('gantt')}/gantt-${name}.png` });
    }
  };
  await openGantt(page, info);
  await setDates(page, {
    'Manage tasks with cards': { from: '2026-11-05', to: '2026-11-11' },
    'Create your own board': { from: '2026-11-16', to: '2026-11-20' },
    'Create a new view': { from: '2026-11-30', to: '2026-12-02' },
  });
  await expect(bars(page)).toHaveCount(7);
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    if (scheme === 'dark') await expect(page.locator('html')).toHaveClass(/\bdark\b/);
    else await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
    await shoot(`${info.project.name}-${scheme}`);
  }
  await page.emulateMedia({ colorScheme: 'light' });

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByTestId('gantt-today-line')).toBeVisible();
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBe(0);
  await page.getByTestId('gantt-today').click();
  await shoot(`390-${info.project.name}`);
});
