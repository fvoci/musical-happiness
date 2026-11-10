/**
 * Dragging the columns themselves: a second sortable on the same board, with
 * its own scope, whose items are the columns and whose handles are their
 * headings. The row of columns runs along x.
 *
 * Every test ends by reading the order of the columns from the page, and the
 * cards in them where a column drag could have disturbed them.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import {
  board,
  card,
  cardsIn,
  columnsOf,
  expectNoDrag,
  expectTarget,
  Finger,
  grab,
  heading,
  hover,
  list,
  nextFrame,
  notice,
  numbered,
  open,
  overlay,
  pickUp,
  pointOn,
  scrollOf,
} from './support.ts';

const COLUMNS = ['backlog', 'ready', 'doing', 'review', 'done', 'archive'];

const column = (page: Page, id: string): Locator => page.locator(`[data-column="${id}"]`);
const columnAnnouncer = (page: Page): Locator => page.getByTestId('column-announcer');

/** A point on a column as it lies now: `across` of the way from its left, at the height of the headings. */
async function pointAcross(
  page: Page,
  id: string,
  across: number,
): Promise<{ x: number; y: number }> {
  const box = await column(page, id).boundingBox();
  if (!box) throw new Error('the column is not on the page');
  return { x: box.x + box.width * across, y: box.y + 20 };
}

/** Moves onto a column as it lies now. Twice: the first move may open a gap that shifts it. */
async function hoverColumn(page: Page, id: string, across: number): Promise<void> {
  for (let pass = 0; pass < 2; pass += 1) {
    const to = await pointAcross(page, id, across);
    await page.mouse.move(to.x, to.y, { steps: 8 });
    await nextFrame(page);
  }
}

async function expectColumnIndex(page: Page, index: number): Promise<void> {
  await expect(board(page)).toHaveAttribute('data-column-index', String(index));
}

async function expectCardsUntouched(page: Page): Promise<void> {
  expect(await cardsIn(page, 'backlog')).toEqual(numbered('backlog', 1, 12));
  expect(await cardsIn(page, 'ready')).toEqual(numbered('ready', 1, 30));
  expect(await cardsIn(page, 'doing')).toEqual(numbered('doing', 1, 6));
}

test.describe('columns', () => {
  test.beforeEach(async ({ page }) => {
    await open(page);
    expect(await columnsOf(page)).toEqual(COLUMNS);
  });

  // ---------------------------------------------------------------- mouse

  test.describe('mouse', () => {
    test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');

    test('a column is dragged by its heading to a new place, and its cards stay as they are', async ({
      page,
    }) => {
      await grab(page, heading(page, 'backlog'));
      // The column under the pointer is a copy; the original is out of the way, and its gap is a column's.
      await expect(column(page, 'backlog')).toBeHidden();
      await expect(page.locator('[data-column-gap]')).toHaveCount(1);
      await expect(page.locator('[data-drag-gap]')).toHaveCount(0);
      await expect(board(page)).not.toHaveAttribute('data-drop-list');

      // Past the middle of a column is the place after it.
      await hoverColumn(page, 'doing', 0.8);
      await expectColumnIndex(page, 2);
      await page.mouse.up();

      expect(await columnsOf(page)).toEqual([
        'ready',
        'doing',
        'backlog',
        'review',
        'done',
        'archive',
      ]);
      await expectCardsUntouched(page);
      await expect(page.getByTestId('last-move')).toHaveText('column backlog: 0 to 2');
      await expect(page.getByTestId('moves')).toHaveText('1');
      await expect(columnAnnouncer(page)).toHaveText('Dropped column Backlog, 3 of 6.');
      await expectNoDrag(page);
      // The button going up on a heading opened nothing.
      await expect(page.getByTestId('opened')).toHaveText('none');
    });

    test('before the middle of a column is the place before it, also toward the start', async ({
      page,
    }) => {
      await grab(page, heading(page, 'review'));
      await hoverColumn(page, 'ready', 0.2);
      await expectColumnIndex(page, 1);
      await hoverColumn(page, 'backlog', 0.2);
      await expectColumnIndex(page, 0);
      await page.mouse.up();
      expect(await columnsOf(page)).toEqual([
        'review',
        'backlog',
        'ready',
        'doing',
        'done',
        'archive',
      ]);
      await expectCardsUntouched(page);
    });

    test('a press on a card drags the card and no column', async ({ page }) => {
      await pickUp(page, 'backlog-2');
      await expect(page.locator('[data-column-gap]')).toHaveCount(0);
      await expect(board(page)).not.toHaveAttribute('data-column-index');
      await expect(overlay(page)).toHaveCount(1);
      await hover(page, 'ready-3', 0.25);
      await expectTarget(page, 'ready', 2);
      await page.mouse.up();
      expect(await columnsOf(page)).toEqual(COLUMNS);
      expect((await cardsIn(page, 'ready'))[2]).toBe('backlog-2');
    });

    test('a column is held by its heading only: a press between its cards drags nothing', async ({
      page,
    }) => {
      const body = await list(page, 'backlog').boundingBox();
      if (!body) throw new Error('the column is not on the page');
      // In the padding of the list, beside the cards.
      await page.mouse.move(body.x + 3, body.y + 60);
      await page.mouse.down();
      await page.mouse.move(body.x + 200, body.y + 120, { steps: 6 });
      await expect(overlay(page)).toHaveCount(0);
      await page.mouse.up();
      await expectNoDrag(page);
      expect(await columnsOf(page)).toEqual(COLUMNS);
    });

    test('a click on a heading drags nothing', async ({ page }) => {
      await heading(page, 'ready').click();
      await expectNoDrag(page);
      await expect(page.getByTestId('moves')).toHaveText('0');
      expect(await columnsOf(page)).toEqual(COLUMNS);
    });

    test('a column held at the right edge scrolls the board and lands at the far end', async ({
      page,
    }) => {
      const size = page.viewportSize();
      if (!size) throw new Error('no viewport');
      await expect(column(page, 'archive')).not.toBeInViewport();
      await grab(page, heading(page, 'backlog'));
      const y = (await pointOn(heading(page, 'ready'))).y;
      await page.mouse.move(size.width - 6, y, { steps: 12 });
      await expect.poll(async () => (await scrollOf(board(page))).atEnd).toBe(true);
      // The pointer rested; the place went along the row with the scrolling.
      await expectColumnIndex(page, 5);
      await page.mouse.up();

      expect(await columnsOf(page)).toEqual([
        'ready',
        'doing',
        'review',
        'done',
        'archive',
        'backlog',
      ]);
      expect((await scrollOf(board(page))).left).toBeGreaterThan(0);
      await expect(column(page, 'backlog')).toBeInViewport();
      // The lists of cards inside the columns did not scroll with it.
      expect((await scrollOf(list(page, 'ready'))).top).toBe(0);
      await expectCardsUntouched(page);
    });

    test('Escape puts the column back, and the board where it was', async ({ page }) => {
      const size = page.viewportSize();
      if (!size) throw new Error('no viewport');
      await grab(page, heading(page, 'backlog'));
      const y = (await pointOn(heading(page, 'ready'))).y;
      await page.mouse.move(size.width - 6, y, { steps: 12 });
      await expect.poll(async () => (await scrollOf(board(page))).left).toBeGreaterThan(200);

      await page.keyboard.press('Escape');
      await expectNoDrag(page);
      expect(await columnsOf(page)).toEqual(COLUMNS);
      expect((await scrollOf(board(page))).left).toBe(0);
      await expect(columnAnnouncer(page)).toHaveText(
        'Cancelled. Column Backlog is back at 1 of 6.',
      );
      await page.mouse.up();
      await expect(page.getByTestId('moves')).toHaveText('0');
    });

    test('a column let go outside the board is not moved', async ({ page }) => {
      await grab(page, heading(page, 'backlog'));
      await hoverColumn(page, 'doing', 0.8);
      await expectColumnIndex(page, 2);
      await page.mouse.move(500, 20, { steps: 8 });
      await expect(board(page)).toHaveAttribute('data-outside', '');
      await expect(notice(page)).toHaveText('Let go to cancel.');
      await expectColumnIndex(page, 0);
      await page.mouse.up();

      await expectNoDrag(page);
      expect(await columnsOf(page)).toEqual(COLUMNS);
      await expect(page.getByTestId('moves')).toHaveText('0');
      await expect(columnAnnouncer(page)).toHaveText(
        'Cancelled. Column Backlog is back at 1 of 6.',
      );
    });
  });

  // ---------------------------------------------------------------- touch

  test.describe('touch', () => {
    test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');

    test('a long press on a heading picks the column up; the board scrolls and it lands one place on', async ({
      page,
    }) => {
      const size = page.viewportSize();
      if (!size) throw new Error('no viewport');
      const finger = await Finger.on(page);
      const y = (await pointOn(heading(page, 'backlog'))).y;
      await finger.rest(heading(page, 'backlog'));
      await expect(column(page, 'backlog')).toBeHidden();
      await expectColumnIndex(page, 0);

      // The middle of the next column is beyond the screen. Held near the
      // right edge, where the board scrolls slowly, until it has come under
      // the finger.
      await finger.move({ x: size.width - 36, y });
      await expectColumnIndex(page, 1);
      await finger.up();

      expect(await columnsOf(page)).toEqual([
        'ready',
        'backlog',
        'doing',
        'review',
        'done',
        'archive',
      ]);
      await expectCardsUntouched(page);
      await expect(page.getByTestId('opened')).toHaveText('none');
      await expectNoDrag(page);
    });

    test('a swipe over the headings scrolls the board and drags nothing', async ({ page }) => {
      const finger = await Finger.on(page);
      const start = await pointOn(heading(page, 'backlog'));
      await finger.down({ x: start.x + 80, y: start.y });
      await finger.move({ x: start.x - 180, y: start.y }, 10);
      await finger.up();

      await expect.poll(async () => (await scrollOf(board(page))).left).toBeGreaterThan(100);
      await expectNoDrag(page);
      expect(await columnsOf(page)).toEqual(COLUMNS);
      await expect(page.getByTestId('moves')).toHaveText('0');
    });

    test('a long press on a card still picks up the card, not its column', async ({ page }) => {
      const finger = await Finger.on(page);
      await finger.pickUp('backlog-1');
      await expect(page.locator('[data-column-gap]')).toHaveCount(0);
      await expect(page.locator('[data-drag-gap]')).toHaveCount(1);
      await finger.hover('backlog-3', 0.8);
      await finger.up();
      expect(await columnsOf(page)).toEqual(COLUMNS);
      expect((await cardsIn(page, 'backlog')).slice(0, 3)).toEqual([
        'backlog-2',
        'backlog-3',
        'backlog-1',
      ]);
    });
  });

  // ------------------------------------------------------------- keyboard

  test.describe('keyboard', () => {
    test('Space on a heading picks the column up, the arrows move it, Space drops it', async ({
      page,
    }) => {
      await heading(page, 'backlog').focus();
      await page.keyboard.press('Space');
      await expect(columnAnnouncer(page)).toHaveText('Picked up column Backlog, 1 of 6.');
      await expect(board(page)).toHaveAttribute('data-dragging', 'keyboard');
      await expect(heading(page, 'backlog')).toBeFocused();

      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await expectColumnIndex(page, 2);
      await expect(columnAnnouncer(page)).toHaveText('Column Backlog, 3 of 6.');
      // The column stands where it would land, and still holds the focus. Nothing is moved yet.
      expect((await columnsOf(page)).slice(0, 3)).toEqual(['ready', 'doing', 'backlog']);
      await expect(heading(page, 'backlog')).toBeFocused();
      await expect(page.getByTestId('moves')).toHaveText('0');

      // Up and down lead nowhere in a single row.
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('ArrowUp');
      await expectColumnIndex(page, 2);
      await page.keyboard.press('ArrowLeft');
      await expectColumnIndex(page, 1);
      await page.keyboard.press('End');
      await expectColumnIndex(page, 5);
      await page.keyboard.press('ArrowRight');
      await expectColumnIndex(page, 5);
      await page.keyboard.press('Home');
      await page.keyboard.press('ArrowLeft');
      await expectColumnIndex(page, 0);
      await page.keyboard.press('ArrowRight');

      await page.keyboard.press('Space');
      await expect(columnAnnouncer(page)).toHaveText('Dropped column Backlog, 2 of 6.');
      expect(await columnsOf(page)).toEqual([
        'ready',
        'backlog',
        'doing',
        'review',
        'done',
        'archive',
      ]);
      await expect(page.getByTestId('last-move')).toHaveText('column backlog: 0 to 1');
      await expect(heading(page, 'backlog')).toBeFocused();
      await expect(board(page)).not.toHaveAttribute('data-dragging');
      await expectCardsUntouched(page);
    });

    test('a column carried out of sight is followed by the board', async ({ page }) => {
      await heading(page, 'backlog').focus();
      await page.keyboard.press('Space');
      await page.keyboard.press('End');
      await expectColumnIndex(page, 5);
      await expect(heading(page, 'backlog')).toBeInViewport();
      await expect(heading(page, 'backlog')).toBeFocused();
      await page.keyboard.press('Enter');
      expect((await columnsOf(page)).at(-1)).toBe('backlog');
      await expect(heading(page, 'backlog')).toBeFocused();
    });

    test('Escape puts a carried column back', async ({ page }) => {
      await heading(page, 'ready').focus();
      await page.keyboard.press('Space');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await expectColumnIndex(page, 3);
      await page.keyboard.press('Escape');
      await expect(columnAnnouncer(page)).toHaveText('Cancelled. Column Ready is back at 2 of 6.');
      expect(await columnsOf(page)).toEqual(COLUMNS);
      await expect(page.getByTestId('moves')).toHaveText('0');
      await expect(heading(page, 'ready')).toBeFocused();
      await expectNoDrag(page);
    });

    test('Tab does not leave a carried column behind', async ({ page }) => {
      await heading(page, 'ready').focus();
      await page.keyboard.press('Space');
      await page.keyboard.press('Tab');
      await expect(heading(page, 'ready')).toBeFocused();
      await expect(board(page)).toHaveAttribute('data-dragging', 'keyboard');
      await page.keyboard.press('Escape');
      await expectNoDrag(page);
    });

    test('Space on a card picks up the card, and Space on a heading the column: never both', async ({
      page,
    }) => {
      await card(page, 'backlog-2').focus();
      await page.keyboard.press('Space');
      await expectTarget(page, 'backlog', 1);
      await expect(board(page)).not.toHaveAttribute('data-column-index');
      await expect(columnAnnouncer(page)).toHaveText('');
      await page.keyboard.press('Escape');

      await heading(page, 'backlog').focus();
      await page.keyboard.press('Space');
      await expectColumnIndex(page, 0);
      await expect(board(page)).not.toHaveAttribute('data-drop-list');
      await expect(page.locator('[data-drag-carried]')).toHaveCount(1);
      await page.keyboard.press('Escape');
      await expectNoDrag(page);
      expect(await columnsOf(page)).toEqual(COLUMNS);
    });
  });
});

// --------------------------------------------------------- right to left

test.describe('columns on a page that runs right to left', () => {
  test.beforeEach(async ({ page }) => {
    await open(page, '?dir=rtl');
    // The first column is the one at the right.
    const first = await column(page, 'backlog').boundingBox();
    const second = await column(page, 'ready').boundingBox();
    expect(first?.x ?? 0).toBeGreaterThan(second?.x ?? Infinity);
  });

  test('the mouse: past the middle of a column, toward the left, is the place after it', async ({
    page,
    hasTouch,
  }) => {
    test.skip(hasTouch, 'the mouse is tested in the desktop project');
    await grab(page, heading(page, 'backlog'));
    await hoverColumn(page, 'doing', 0.2);
    await expectColumnIndex(page, 2);
    await hoverColumn(page, 'ready', 0.2);
    await expectColumnIndex(page, 1);
    // Its right part is before its middle here: the place before it.
    await hoverColumn(page, 'ready', 0.8);
    await expectColumnIndex(page, 0);
    await hoverColumn(page, 'doing', 0.2);
    await expectColumnIndex(page, 2);
    await page.mouse.up();
    expect(await columnsOf(page)).toEqual([
      'ready',
      'doing',
      'backlog',
      'review',
      'done',
      'archive',
    ]);
  });

  test('the keyboard: the left arrow goes on along the row, the right arrow back', async ({
    page,
  }) => {
    await heading(page, 'backlog').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowRight');
    await expectColumnIndex(page, 0);
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowLeft');
    await expectColumnIndex(page, 2);
    await page.keyboard.press('ArrowRight');
    await expectColumnIndex(page, 1);
    await page.keyboard.press('Space');
    expect(await columnsOf(page)).toEqual([
      'ready',
      'backlog',
      'doing',
      'review',
      'done',
      'archive',
    ]);
  });

  test('a card carried with the keyboard goes to the next column with the left arrow', async ({
    page,
  }) => {
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowLeft');
    await expectTarget(page, 'ready', 1);
    await page.keyboard.press('Space');
    expect((await cardsIn(page, 'ready')).slice(0, 3)).toEqual(['ready-1', 'backlog-2', 'ready-2']);
  });

  test('a card dragged with the mouse lands where the pointer is', async ({ page, hasTouch }) => {
    test.skip(hasTouch, 'the mouse is tested in the desktop project');
    await pickUp(page, 'backlog-2');
    await hover(page, 'ready-3', 0.25);
    await expectTarget(page, 'ready', 2);
    await page.mouse.up();
    expect((await cardsIn(page, 'ready')).slice(0, 4)).toEqual([
      'ready-1',
      'ready-2',
      'backlog-2',
      'ready-3',
    ]);
  });
});
