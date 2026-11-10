/**
 * What the product owner decided about dragging cards, in a browser: letting
 * go outside the board cancels, a sorted view lets only the column be chosen
 * and says so, and a long press buzzes.
 */
import { expect, test, type Page } from '@playwright/test';

import { DRAG_SETTINGS } from '../../src/lib/drag/settings.ts';

import {
  announcer,
  board,
  card,
  cardsIn,
  expectNothingMoved,
  expectTarget,
  Finger,
  hover,
  list,
  notice,
  numbered,
  open,
  overlay,
  pickUp,
  pointOn,
  scrollOf,
} from './support.ts';

const SORTED = 'This view is sorted: choose a column, the order decides the place.';
const OUTSIDE = 'Let go to cancel.';

// ---------------------------------------------------- outside the board

test.describe('outside the board', () => {
  test.beforeEach(async ({ page }) => {
    await open(page);
  });

  test.describe('mouse', () => {
    test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');

    test('letting go outside cancels: order and scrolling are as before, and it is announced', async ({
      page,
    }) => {
      const size = page.viewportSize();
      if (!size) throw new Error('no viewport');
      await pickUp(page, 'backlog-2');
      await hover(page, 'ready-5', 0.5);
      await expect(board(page)).toHaveAttribute('data-drop-list', 'ready');
      await expect(notice(page)).toHaveCount(0);
      // Held at the right edge, inside the board: the board scrolls.
      const middle = (await pointOn(board(page))).y;
      await page.mouse.move(size.width - 6, middle, { steps: 12 });
      await expect.poll(async () => (await scrollOf(board(page))).left).toBeGreaterThan(200);

      // Up onto the heading of the page: no longer on the board.
      await page.mouse.move(size.width - 6, 20, { steps: 8 });
      await expect(board(page)).toHaveAttribute('data-outside', '');
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'outside');
      await expect(notice(page)).toHaveText(OUTSIDE);
      // The gap is back where the card came from.
      await expectTarget(page, 'backlog', 1);
      // The pointer is still within reach of the right edge, and nothing scrolls.
      const held = (await scrollOf(board(page))).left;
      await page.waitForTimeout(300);
      expect((await scrollOf(board(page))).left).toBe(held);

      await page.mouse.up();
      await expectNothingMoved(page);
      expect((await scrollOf(board(page))).left).toBe(0);
      await expect(announcer(page)).toHaveText(
        'Cancelled. Backlog 2: Dark appearance is back at position 2 of 12 in Backlog.',
      );
      await expect(notice(page)).toHaveCount(0);
      // The button going up was not a click on anything, and the next press is an ordinary one.
      await expect(page.getByTestId('opened')).toHaveText('none');
      await card(page, 'backlog-2').click();
      await expect(page.getByTestId('opened')).toHaveText('backlog-2');
    });

    test('below the board is outside as well', async ({ page }) => {
      const size = page.viewportSize();
      if (!size) throw new Error('no viewport');
      await pickUp(page, 'backlog-2');
      await hover(page, 'ready-3', 0.25);
      await expectTarget(page, 'ready', 2);
      await page.mouse.move(400, size.height - 4, { steps: 8 });
      await expect(board(page)).toHaveAttribute('data-outside', '');
      await page.mouse.up();
      await expectNothingMoved(page);
    });

    test('a pointer that comes back onto the board drags on', async ({ page }) => {
      await pickUp(page, 'backlog-2');
      await page.mouse.move(300, 20, { steps: 8 });
      await expect(board(page)).toHaveAttribute('data-outside', '');
      await expect(overlay(page)).toBeVisible();

      await hover(page, 'ready-3', 0.25);
      await expect(board(page)).not.toHaveAttribute('data-outside');
      await expect(notice(page)).toHaveCount(0);
      await expectTarget(page, 'ready', 2);
      await page.mouse.up();
      expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', ...numbered('backlog', 3, 12)]);
      expect((await cardsIn(page, 'ready')).slice(0, 4)).toEqual([
        'ready-1',
        'ready-2',
        'backlog-2',
        'ready-3',
      ]);
    });
  });

  test.describe('touch', () => {
    test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');

    test('a finger lifted above the board cancels', async ({ page }) => {
      const finger = await Finger.on(page);
      await finger.pickUp('backlog-1');
      await finger.hover('backlog-3', 0.8);
      await expectTarget(page, 'backlog', 2);

      await finger.move({ x: 200, y: 30 });
      await expect(board(page)).toHaveAttribute('data-outside', '');
      await expect(notice(page)).toHaveText(OUTSIDE);
      await expectTarget(page, 'backlog', 0);
      await finger.up();

      await expectNothingMoved(page);
      await expect(announcer(page)).toContainText('Cancelled. Backlog 1:');
      await expect(page.getByTestId('opened')).toHaveText('none');
    });
  });
});

// ------------------------------------------------------- a sorted view

test.describe('a sorted view', () => {
  // By task, then by id: in Ready three cards say "Dark appearance" and the
  // next four "Filter by date", which is also what Backlog 5 says.
  const READY_WITH_BACKLOG_5 = ['ready-7', 'ready-15', 'ready-23', 'backlog-5', 'ready-2'];

  test.beforeEach(async ({ page }) => {
    await open(page, '?sorted=1');
    expect((await cardsIn(page, 'backlog')).slice(0, 4)).toEqual([
      'backlog-2',
      'backlog-10',
      'backlog-5',
      'backlog-8',
    ]);
  });

  test.describe('mouse', () => {
    test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');

    test('a card dropped on another column goes where the order puts it, wherever the pointer is', async ({
      page,
    }) => {
      await pickUp(page, 'backlog-5');
      await expect(notice(page)).toHaveText(SORTED);
      await expect(announcer(page)).toHaveText(SORTED);

      await hover(page, 'ready-18', 0.8);
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'list');
      await expectTarget(page, 'ready', 3);
      // The one gap is at the place of the order, not under the pointer.
      await expect(page.locator('[data-drag-gap]')).toHaveCount(1);
      await expect(list(page, 'ready').locator('> *').nth(3)).toHaveAttribute('data-drag-gap');
      await hover(page, 'ready-7', 0.2);
      await expectTarget(page, 'ready', 3);
      await expect(notice(page)).toHaveText(SORTED);
      await page.mouse.up();

      expect((await cardsIn(page, 'ready')).slice(0, 5)).toEqual(READY_WITH_BACKLOG_5);
      expect(await cardsIn(page, 'backlog')).not.toContain('backlog-5');
      // The move the page was handed names the column and no index.
      await expect(page.getByTestId('last-move')).toHaveText(
        'backlog-5: backlog 2 to ready, sorted',
      );
      await expect(announcer(page)).toHaveText(
        'Dropped Backlog 5: Filter by date, position 4 of 31 in Ready.',
      );
      await expect(notice(page)).toHaveCount(0);
    });

    test('within its own column a card stays where it is and a drop changes nothing', async ({
      page,
    }) => {
      const before = await cardsIn(page, 'backlog');
      await pickUp(page, 'backlog-5');
      await hover(page, 'backlog-1', 0.8);
      await expect(notice(page)).toHaveText(SORTED);
      await expectTarget(page, 'backlog', 2);
      await page.mouse.up();

      await expect(page.getByTestId('moves')).toHaveText('0');
      expect(await cardsIn(page, 'backlog')).toEqual(before);
    });
  });

  test.describe('touch', () => {
    test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');

    test('a finger sees the notice and cannot place the card either', async ({ page }) => {
      const before = await cardsIn(page, 'backlog');
      const finger = await Finger.on(page);
      await finger.pickUp('backlog-2');
      await expect(notice(page)).toHaveText(SORTED);
      await finger.hover('backlog-8', 0.8);
      await expectTarget(page, 'backlog', 0);
      await finger.up();
      await expect(page.getByTestId('moves')).toHaveText('0');
      expect(await cardsIn(page, 'backlog')).toEqual(before);
    });
  });

  test.describe('keyboard', () => {
    test('the keys choose the column only, and say why when asked for a place', async ({
      page,
    }) => {
      await card(page, 'backlog-5').focus();
      await page.keyboard.press('Space');
      await expect(announcer(page)).toHaveText(
        `Picked up Backlog 5: Filter by date, position 3 of 12 in Backlog. ${SORTED}`,
      );
      await expect(notice(page)).toHaveText(SORTED);

      // Up and down, Home and End lead nowhere; the reason is read out, and
      // read out again when asked again.
      await page.keyboard.press('ArrowDown');
      await expect(announcer(page)).toHaveText(SORTED);
      await expectTarget(page, 'backlog', 2);
      const spoken = await announcer(page).evaluate((element) => element.textContent);
      await page.keyboard.press('ArrowUp');
      await expect(announcer(page)).toHaveText(SORTED);
      expect(await announcer(page).evaluate((element) => element.textContent)).not.toBe(spoken);
      await page.keyboard.press('End');
      await page.keyboard.press('Home');
      await expectTarget(page, 'backlog', 2);

      await page.keyboard.press('ArrowRight');
      await expectTarget(page, 'ready', 3);
      await expect(announcer(page)).toHaveText(
        'Backlog 5: Filter by date, position 4 of 31 in Ready.',
      );
      await expect(page.locator('[data-drag-carried]')).toBeFocused();
      await page.keyboard.press('ArrowDown');
      await expectTarget(page, 'ready', 3);
      await expect(page.getByTestId('moves')).toHaveText('0');

      await page.keyboard.press('Space');
      await expect(announcer(page)).toHaveText(
        'Dropped Backlog 5: Filter by date, position 4 of 31 in Ready.',
      );
      expect((await cardsIn(page, 'ready')).slice(0, 5)).toEqual(READY_WITH_BACKLOG_5);
      await expect(page.getByTestId('last-move')).toHaveText(
        'backlog-5: backlog 2 to ready, sorted',
      );
      await expect(card(page, 'backlog-5')).toBeFocused();
      await expect(notice(page)).toHaveCount(0);
    });

    test('back in its own column the card is where it was, and a drop there changes nothing', async ({
      page,
    }) => {
      const before = await cardsIn(page, 'backlog');
      await card(page, 'backlog-5').focus();
      await page.keyboard.press('Space');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowLeft');
      await expectTarget(page, 'backlog', 2);
      await page.keyboard.press('Enter');
      await expect(page.getByTestId('moves')).toHaveText('0');
      expect(await cardsIn(page, 'backlog')).toEqual(before);
    });
  });

  test('the switch on the page turns the sorted view on and off', async ({ page }) => {
    await page.getByRole('switch', { name: 'Sorted view' }).click();
    expect(await cardsIn(page, 'backlog')).toEqual(numbered('backlog', 1, 12));
    await page.getByRole('switch', { name: 'Sorted view' }).click();
    expect((await cardsIn(page, 'backlog')).slice(0, 2)).toEqual(['backlog-2', 'backlog-10']);
  });
});

// ------------------------------------------------ the buzz of a long press

test.describe('the buzz of a long press', () => {
  /** Loads the page with a `navigator.vibrate` that only writes down what it is asked for. */
  async function openRecording(page: Page): Promise<void> {
    await page.addInitScript(() => {
      const asked: unknown[] = [];
      Object.assign(window, { spikeBuzzes: asked });
      Object.defineProperty(Navigator.prototype, 'vibrate', {
        configurable: true,
        value: (pattern: unknown) => {
          asked.push(pattern);
          return true;
        },
      });
    });
    await open(page);
  }

  const buzzes = (page: Page): Promise<unknown[]> =>
    page.evaluate(() => (globalThis as unknown as { spikeBuzzes: unknown[] }).spikeBuzzes);

  test.describe('touch', () => {
    test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');

    test('a finger that has rested long enough buzzes once, for as long as the settings say', async ({
      page,
    }) => {
      await openRecording(page);
      const finger = await Finger.on(page);
      await finger.pickUp('backlog-1');
      expect(await buzzes(page)).toEqual([DRAG_SETTINGS.touchVibration]);

      await finger.hover('backlog-3', 0.8);
      await finger.up();
      expect(await buzzes(page)).toEqual([DRAG_SETTINGS.touchVibration]);
      expect((await cardsIn(page, 'backlog')).slice(0, 3)).toEqual([
        'backlog-2',
        'backlog-3',
        'backlog-1',
      ]);
    });

    test('a finger that scrolls, or lifts before the wait is over, does not buzz', async ({
      page,
    }) => {
      await openRecording(page);
      const finger = await Finger.on(page);
      const start = await pointOn(card(page, 'backlog-4'));
      await finger.down(start);
      await finger.move({ x: start.x, y: start.y - 200 }, 10);
      await finger.up();
      await finger.down(start);
      await finger.up();
      await page.waitForTimeout(DRAG_SETTINGS.touchDelay + 200);
      expect(await buzzes(page)).toEqual([]);
    });

    test('a browser without navigator.vibrate drags all the same', async ({ page }) => {
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.addInitScript(() => {
        Reflect.deleteProperty(Navigator.prototype, 'vibrate');
      });
      await open(page);
      expect(await page.evaluate(() => 'vibrate' in navigator)).toBe(false);

      const finger = await Finger.on(page);
      await finger.pickUp('backlog-1');
      await finger.hover('backlog-3', 0.8);
      await finger.up();
      expect((await cardsIn(page, 'backlog')).slice(0, 3)).toEqual([
        'backlog-2',
        'backlog-3',
        'backlog-1',
      ]);
      expect(errors).toEqual([]);
    });

    test('a browser that refuses to vibrate drags all the same', async ({ page }) => {
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.addInitScript(() => {
        Object.defineProperty(Navigator.prototype, 'vibrate', {
          configurable: true,
          value: () => {
            throw new Error('no vibration here');
          },
        });
      });
      await open(page);

      const finger = await Finger.on(page);
      await finger.pickUp('backlog-1');
      await finger.hover('backlog-3', 0.8);
      await finger.up();
      expect((await cardsIn(page, 'backlog')).slice(0, 3)).toEqual([
        'backlog-2',
        'backlog-3',
        'backlog-1',
      ]);
      expect(errors).toEqual([]);
    });
  });

  test('the mouse and the keyboard do not buzz', async ({ page, hasTouch }) => {
    await openRecording(page);
    if (!hasTouch) {
      await pickUp(page, 'backlog-2');
      await hover(page, 'ready-3', 0.25);
      await page.mouse.up();
      await expect(page.getByTestId('moves')).toHaveText('1');
    }
    await card(page, 'backlog-4').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Space');
    await expect(page.getByTestId('moves')).toHaveText(hasTouch ? '1' : '2');
    expect(await buzzes(page)).toEqual([]);
  });
});
