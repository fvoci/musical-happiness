/**
 * The dependencies of a Gantt view on the in-memory backend: the card
 * relation the header picks, one arrow for each edge, from the end of one
 * bar to the start of the other, and what is out of order or on a loop.
 */
import { expect, test, type Page } from '@playwright/test';

import {
  addRelation,
  arrowEnds,
  arrows,
  bar,
  bars,
  chooseDependencies,
  expectDependencies,
  gantt,
  openGantt,
  openRelated,
  setAfter,
  setDates,
  storedFields,
  type Dev,
} from './gantt.support.ts';

test.use({ timezoneId: 'Asia/Seoul', locale: 'en-US' });

test('a new Gantt view reads the first card relation, and draws one arrow for each edge', async ({
  page,
}, info) => {
  await openRelated(page, info);
  await expectDependencies(page, 'After');
  expect((await storedFields(page)).dependencyPropertyId).toBe('after');
  // Three edges between cards that have a bar; the deleted card, the card
  // itself and the card without a date give none.
  await expect(gantt(page)).toHaveAttribute('data-edges', '3');
  await expect(arrows(page)).toHaveCount(3);
  const ends = await arrows(page).evaluateAll((drawn) =>
    drawn.map((arrow) => [arrow.getAttribute('data-from'), arrow.getAttribute('data-to')]),
  );
  const ids = await page.evaluate(() => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    return Object.fromEntries(store.cards.map((card) => [card.title, card.id]));
  });
  expect(ends).toEqual([
    [ids['Create a new card'], ids['Share a board']],
    [ids['Share a board'], ids['Filter and sort cards']],
    [ids['Drag cards'], ids['Filter and sort cards']],
  ]);
  // A bar says which cards it comes after.
  await expect(bar(page, 'Filter and sort cards')).toHaveAccessibleName(
    'Filter and sort cards, December 3 – December 4. After Share a board and Drag cards.',
  );
  await expect(bar(page, 'Drag cards')).toHaveAccessibleName('Drag cards, November 12');
});

test('an arrow goes from the end of the card that comes first to the start of the one after', async ({
  page,
}, info) => {
  await openRelated(page, info);
  await expect(arrows(page)).toHaveCount(3);
  /** The arrow between two cards leaves the end of the first bar and reaches the start of the second. */
  async function joins(from: string, to: string): Promise<void> {
    await bar(page, to).scrollIntoViewIfNeeded();
    const line = await arrowEnds(page, from, to);
    const first = await bar(page, from).boundingBox();
    const second = await bar(page, to).boundingBox();
    if (!first || !second) throw new Error('no bars to measure');
    expect(Math.abs(line.start.x - (first.x + first.width)), `${from} → ${to}`).toBeLessThan(1.5);
    expect(Math.abs(line.start.y - (first.y + first.height / 2))).toBeLessThan(1.5);
    expect(Math.abs(line.end.x - second.x)).toBeLessThan(1.5);
    expect(Math.abs(line.end.y - (second.y + second.height / 2))).toBeLessThan(1.5);
  }
  // One that turns back (the second card starts two days after the first ends, less than
  // two gaps at this zoom), and one that goes straight in, two rows down.
  await joins('Create a new card', 'Share a board');
  await joins('Drag cards', 'Filter and sort cards');
  // Closer in, with a heading of a group between the rows, the arrows still meet the bars.
  await page.getByTestId('gantt-zoom-in').click();
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'day');
  await page.evaluate(async () => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    const select = store.board.cardProperties.find((property) => property.type === 'select');
    const view = store.views.find((each) => each.fields.viewType === 'gantt');
    if (!select || !view) throw new Error('nothing to group by');
    await store.setViewFields(view.id, { groupById: select.id });
  });
  await expect(gantt(page).locator('[data-group]').first()).toBeVisible();
  await joins('Create a new card', 'Share a board');
  await joins('Share a board', 'Filter and sort cards');
});

test('"dependencies" in the header turns the arrows off and on, and the view keeps the choice', async ({
  page,
}, info) => {
  await openRelated(page, info);
  await expect(arrows(page)).toHaveCount(3);
  await chooseDependencies(page, '(none)');
  await expect(arrows(page)).toHaveCount(0);
  await expect(page.getByTestId('gantt-arrows')).toHaveCount(0);
  await expectDependencies(page, 'None');
  await expect.poll(async () => (await storedFields(page)).dependencyPropertyId).toBe('');
  await chooseDependencies(page, 'after');
  await expect(arrows(page)).toHaveCount(3);
  await expect.poll(async () => (await storedFields(page)).dependencyPropertyId).toBe('after');
  // One undo takes the last choice back.
  await page.evaluate(() => (window as unknown as { fvociDev: Dev }).fvociDev.workspace.undoLast());
  await expect(arrows(page)).toHaveCount(0);
  await expect.poll(async () => (await storedFields(page)).dependencyPropertyId).toBe('');
});

test('a Gantt view made on a board without a card relation has no arrows until one is chosen', async ({
  page,
}, info) => {
  await openGantt(page, info);
  await expectDependencies(page, 'None');
  expect((await storedFields(page)).dependencyPropertyId).toBeUndefined();
  await addRelation(page);
  await setAfter(page, { 'Share a board': ['Create a new card'] });
  await expect(arrows(page)).toHaveCount(0);
  await chooseDependencies(page, 'after');
  await expect(arrows(page)).toHaveCount(1);
  await expect(gantt(page)).toHaveAttribute('data-edges', '1');
  // The first choice taken back: the view names no relation again, as before it.
  await page.evaluate(() => (window as unknown as { fvociDev: Dev }).fvociDev.workspace.undoLast());
  await expect(arrows(page)).toHaveCount(0);
  await expectDependencies(page, 'None');
  await expect.poll(async () => 'dependencyPropertyId' in (await storedFields(page))).toBe(false);
});

test('the arrows of a card go with it when it is deleted', async ({ page }, info) => {
  await openRelated(page, info);
  await expect(arrows(page)).toHaveCount(3);
  await page.evaluate(async () => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    const gone = store.cards.find((card) => card.title === 'Share a board');
    if (!gone) throw new Error('no card to delete');
    await store.deleteCards([gone.id]);
  });
  // The value of "Filter and sort cards" still names the deleted card: no arrow for it.
  await expect(bars(page)).toHaveCount(3);
  await expect(arrows(page)).toHaveCount(1);
  await expect(gantt(page)).toHaveAttribute('data-edges', '1');
});

// ------------------------------------------- out of order, and loops

async function dashes(page: Page, from: string, to: string): Promise<string | null> {
  const ids = await page.evaluate(() => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    return Object.fromEntries(store.cards.map((card) => [card.title, card.id]));
  });
  return gantt(page)
    .locator(`[data-arrow][data-from="${ids[from] ?? ''}"][data-to="${ids[to] ?? ''}"] path`)
    .first()
    .getAttribute('stroke-dasharray');
}

test('a card that starts before a card it comes after has ended is marked, counted and said', async ({
  page,
}, info) => {
  await openRelated(page, info);
  // "Share a board" starts on the day "Drag cards" ends: that is in order.
  await setAfter(page, { 'Share a board': ['Create a new card', 'Drag cards'] });
  await expect(arrows(page)).toHaveCount(4);
  await expect(gantt(page)).toHaveAttribute('data-late', '0');
  await expect(page.getByTestId('gantt-late')).toHaveCount(0);
  await expect(gantt(page).locator('[data-mark]')).toHaveCount(0);

  // "Filter and sort cards" now starts three days before "Share a board" ends.
  await setDates(page, { 'Filter and sort cards': { from: '2026-11-20', to: '2026-11-21' } });
  await expect(gantt(page)).toHaveAttribute('data-late', '1');
  await expect(arrows(page)).toHaveCount(4);
  await expect(gantt(page).locator('[data-arrow][data-late]')).toHaveCount(1);
  // Not by colour alone: the line is dashed, the others are not.
  expect(await dashes(page, 'Share a board', 'Filter and sort cards')).toBe('6 4');
  expect(await dashes(page, 'Drag cards', 'Filter and sort cards')).toBeNull();
  // A mark next to the title, which stays in sight while the days scroll.
  const row = gantt(page)
    .locator('[data-row]')
    .filter({ has: page.locator('[data-row-title]', { hasText: 'Filter and sort cards' }) });
  await expect(row.locator('[data-row-title] [data-mark="late"]')).toBeVisible();
  await expect(gantt(page).locator('[data-mark]')).toHaveCount(1);
  await expect(bar(page, 'Filter and sort cards')).toHaveAccessibleName(
    'Filter and sort cards, November 20 – November 21. After Share a board and Drag cards. Out of order: starts before the end of Share a board.',
  );
  // The row above the chart counts them, and goes to the card.
  const count = page.getByTestId('gantt-late');
  await expect(count).toHaveAccessibleName('Out of order: 1. Show the next.');
  await expect(count.locator('[data-count]')).toHaveText('1');
  await count.click();
  await expect(bar(page, 'Filter and sort cards')).toBeFocused();
  await expect(bar(page, 'Filter and sort cards')).toBeInViewport();

  // Dates are not moved for the reader: the card is where its value says.
  const stored = await page.evaluate(() => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    const due = store.board.cardProperties.find((property) => property.name === 'Due Date');
    const card = store.cards.find((each) => each.title === 'Filter and sort cards');
    return (card as unknown as { fields: { properties: Record<string, unknown> } }).fields
      .properties[due?.id ?? ''];
  });
  expect(stored).toEqual({ from: '2026-11-20', to: '2026-11-21' });

  // Put back in order, nothing is marked.
  await setDates(page, { 'Filter and sort cards': { from: '2026-11-23', to: '2026-11-24' } });
  await expect(page.getByTestId('gantt-late')).toHaveCount(0);
  await expect(gantt(page).locator('[data-mark]')).toHaveCount(0);
});

test('cards on a loop are marked, counted and said, the arrows between them dotted', async ({
  page,
}, info) => {
  await openRelated(page, info);
  await expect(page.getByTestId('gantt-loops')).toHaveCount(0);
  // "Create a new card" → "Share a board" → "Filter and sort cards" → "Create a new card".
  await setAfter(page, { 'Create a new card': ['Filter and sort cards'] });
  await expect(arrows(page)).toHaveCount(4);
  await expect(gantt(page).locator('[data-arrow][data-loop]')).toHaveCount(3);
  // In order and on the loop: dotted. The one that closes it runs backwards in time: dashed.
  expect(await dashes(page, 'Create a new card', 'Share a board')).toBe('0.1 4.5');
  expect(await dashes(page, 'Filter and sort cards', 'Create a new card')).toBe('6 4');
  expect(await dashes(page, 'Drag cards', 'Filter and sort cards')).toBeNull();
  await expect(gantt(page).locator('[data-bar][data-loop]')).toHaveCount(3);
  await expect(bar(page, 'Drag cards')).not.toHaveAttribute('data-loop');
  await expect(bar(page, 'Share a board')).toHaveAccessibleName(
    'Share a board, November 12 – November 23. After Create a new card. On a loop: following what it comes after leads back to it.',
  );
  const count = page.getByTestId('gantt-loops');
  await expect(count).toHaveAccessibleName('On a loop: 3. Show the next.');
  await expect(page.getByTestId('gantt-late')).toHaveAccessibleName(
    'Out of order: 1. Show the next.',
  );
  // The button walks the cards of the loop, from the top and round again.
  const onLoop = gantt(page).locator('[data-bar][data-loop]');
  for (const at of [0, 1, 2, 0]) {
    await count.click();
    await expect(onLoop.nth(at)).toBeFocused();
  }
});

test('a loop through a card without a date still marks the cards that have a bar', async ({
  page,
}, info) => {
  await openRelated(page, info);
  // "Drag cards" ↔ "Create a new view", which has no date and so no bar and no arrow.
  await setAfter(page, { 'Drag cards': ['Create a new view'] });
  await expect(arrows(page)).toHaveCount(3);
  await expect(gantt(page).locator('[data-arrow][data-loop]')).toHaveCount(0);
  await expect(bar(page, 'Drag cards')).toHaveAttribute('data-loop', '');
  await expect(page.getByTestId('gantt-loops')).toHaveAccessibleName(
    'On a loop: 1. Show the next.',
  );
});
