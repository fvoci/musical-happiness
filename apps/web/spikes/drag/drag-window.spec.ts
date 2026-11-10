/**
 * The window under a drag: what happens when its size changes in the middle
 * of one (a device unfolded or turned, a window put beside another), and
 * that it is the kind of pointer, not the width of the window, that decides
 * how a drag begins. A tablet is wide and is touched; a small window on a
 * desktop is narrow and has a mouse.
 */
import { expect, test } from '@playwright/test';

import { DRAG_SETTINGS } from '../../src/lib/drag/settings.ts';

import {
  announcer,
  card,
  cardsIn,
  expectNoDrag,
  expectNothingMoved,
  Finger,
  hover,
  numbered,
  open,
  overlay,
  pickUp,
  pointOn,
} from './support.ts';

// Sizes between a phone and a desktop. Rough figures of a folded and an
// unfolded device and of a tablet; they are not measured on the devices.
const UNFOLDED = { width: 626, height: 890 };
const UNFOLDED_TURNED = { width: 890, height: 626 };
const TABLET_ACROSS = { width: 1180, height: 820 };
const PHONE = { width: 360, height: 740 };

test.describe('the window changes size during a drag', () => {
  test.describe('mouse', () => {
    test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');
    test.use({ viewport: UNFOLDED });

    test('a change of width cancels the drag, and it is announced', async ({ page }) => {
      await open(page);
      await pickUp(page, 'backlog-2');
      await hover(page, 'backlog-6', 0.9);
      await expect(page.locator('[data-drag-gap]')).toHaveCount(1);

      // The device is turned on its side.
      await page.setViewportSize(UNFOLDED_TURNED);
      await expectNoDrag(page);
      await expect(announcer(page)).toContainText('Cancelled.');

      // The button is still down; letting go moves nothing and opens nothing.
      await page.mouse.up();
      await expectNothingMoved(page);
      await expect(page.getByTestId('opened')).toHaveText('none');

      // The next drag works as ever, in the new size.
      await pickUp(page, 'backlog-2');
      await hover(page, 'backlog-4', 0.9);
      await page.mouse.up();
      expect((await cardsIn(page, 'backlog')).slice(0, 4)).toEqual([
        'backlog-1',
        'backlog-3',
        'backlog-4',
        'backlog-2',
      ]);
    });

    test('a little more or less height does not: a bar of the browser sliding away', async ({
      page,
    }) => {
      await open(page);
      await pickUp(page, 'backlog-2');
      await hover(page, 'backlog-4', 0.9);
      await page.setViewportSize({
        width: UNFOLDED.width,
        height: UNFOLDED.height - DRAG_SETTINGS.resizeTolerance + 20,
      });
      await expect(overlay(page)).toBeVisible();
      await page.mouse.up();
      expect((await cardsIn(page, 'backlog')).slice(0, 4)).toEqual([
        'backlog-1',
        'backlog-3',
        'backlog-4',
        'backlog-2',
      ]);
    });

    test('much less height does: another window took half of the screen', async ({ page }) => {
      await open(page);
      await pickUp(page, 'backlog-2');
      await hover(page, 'backlog-4', 0.9);
      await page.setViewportSize({ width: UNFOLDED.width, height: 440 });
      await expectNoDrag(page);
      await page.mouse.up();
      await expectNothingMoved(page);
    });
  });

  test.describe('touch', () => {
    test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');

    test('a device unfolded under a resting finger gives the card up', async ({ page }) => {
      await open(page);
      const finger = await Finger.on(page);
      await finger.pickUp('backlog-2');
      await page.setViewportSize(UNFOLDED);
      await expectNoDrag(page);
      await expect(announcer(page)).toContainText('Cancelled.');
      await finger.up();
      await expectNothingMoved(page);
    });
  });

  test('a card carried with the keyboard is put back too', async ({ page }) => {
    await open(page);
    const size = page.viewportSize();
    if (!size) throw new Error('no viewport');
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowDown');
    await expect(page.locator('[data-drag-carried]')).toHaveCount(1);

    await page.setViewportSize({ width: size.width + 200, height: size.height });
    await expect(page.locator('[data-drag-carried]')).toHaveCount(0);
    await expect(announcer(page)).toContainText('Cancelled.');
    await expectNothingMoved(page);
    // The card has the focus again, where it was.
    await expect(card(page, 'backlog-2')).toBeFocused();
  });

  test('a change of size while nothing is dragged leaves the board as it is', async ({ page }) => {
    await open(page);
    const size = page.viewportSize();
    if (!size) throw new Error('no viewport');
    await page.locator('[data-drag-list="ready"]').evaluate((element) => {
      element.scrollTop = 300;
    });
    await page.setViewportSize({ width: size.width + 200, height: size.height });
    await expectNothingMoved(page);
    // What a column was scrolled to stays: the columns are as tall as before.
    expect(
      await page.locator('[data-drag-list="ready"]').evaluate((element) => element.scrollTop),
    ).toBe(300);
  });
});

test.describe('how a drag begins follows the pointer, not the width of the window', () => {
  test.describe('a mouse in a window as narrow as a phone', () => {
    test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');
    test.use({ viewport: PHONE });

    test('drags at once, after a few pixels, without a wait', async ({ page }) => {
      await open(page);
      const from = await pointOn(card(page, 'backlog-2'));
      await page.mouse.move(from.x, from.y);
      await page.mouse.down();
      const started = Date.now();
      await page.mouse.move(from.x + 6, from.y + 6, { steps: 3 });
      await expect(overlay(page)).toBeVisible();
      // No long press: the drag is there well before a finger's wait would be over.
      expect(Date.now() - started).toBeLessThan(DRAG_SETTINGS.touchDelay);
      await page.keyboard.press('Escape');
      await page.mouse.up();
      await expectNothingMoved(page);
    });
  });

  test.describe('a finger on a screen as wide as a desktop', () => {
    test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');
    test.use({ viewport: TABLET_ACROSS });

    test('scrolls when it moves at once, and drags only after resting', async ({ page }) => {
      await open(page);
      const finger = await Finger.on(page);

      // A swipe up over the cards of a long column: the column scrolls, nothing is dragged.
      const start = await pointOn(card(page, 'ready-6'));
      await finger.down(start);
      await finger.move({ x: start.x, y: start.y - 160 });
      await expect(overlay(page)).toHaveCount(0);
      await finger.up();
      await expect
        .poll(() =>
          page.locator('[data-drag-list="ready"]').evaluate((element) => element.scrollTop),
        )
        .toBeGreaterThan(40);
      await expectNoDrag(page);
      expect(await cardsIn(page, 'ready')).toEqual(numbered('ready', 1, 30));

      // A finger that rests picks the card up.
      await finger.pickUp('backlog-2');
      await finger.cancel();
      await expectNoDrag(page);
    });
  });
});
