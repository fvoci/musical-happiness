/**
 * What the foundation takes although the product's screens for it come
 * later: several selected cards dragged as one, and a zone, a place to drop
 * on that is no list (here a hidden column).
 *
 * The page has only the plainest way of selecting (a switch, then a tap on
 * each card) and of showing the zone; these tests are about what a drag
 * carries and what a drop reports.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import {
  announcer,
  board,
  card,
  cardsIn,
  expectTarget,
  Finger,
  hover,
  numbered,
  open,
  overlay,
  pickUp,
  pointOn,
  scrollOf,
} from './support.ts';

// ------------------------------------------------- several cards at once

test.describe('several cards at once', () => {
  const READY_WITH_TWO = ['ready-1', 'ready-2', 'backlog-2', 'backlog-4', 'ready-3'];

  /** Selects cards, in the order given, with the page's switch on. */
  async function select(page: Page, ...ids: string[]): Promise<void> {
    for (const id of ids) await card(page, id).click();
    await expect(page.getByTestId('selected')).toHaveText(String(ids.length));
    await expect(page.getByTestId('opened')).toHaveText('none');
  }

  test.beforeEach(async ({ page }) => {
    await open(page, '?select=1');
  });

  test.describe('mouse', () => {
    test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');

    test('dragging one of the selected cards moves them all, in the order they had', async ({
      page,
    }) => {
      // Selected the other way round: the order of the board is what counts.
      await select(page, 'backlog-4', 'backlog-2');
      await pickUp(page, 'backlog-4');
      await expect(overlay(page).locator('[data-carry-count]')).toHaveText('2');
      await expect(card(page, 'backlog-2')).toBeHidden();
      await expect(card(page, 'backlog-4')).toBeHidden();
      await expect(page.locator('[data-drag-gap]')).toHaveCount(1);

      await hover(page, 'ready-3', 0.25);
      await expectTarget(page, 'ready', 2);
      await page.mouse.up();

      expect(await cardsIn(page, 'backlog')).toEqual([
        'backlog-1',
        'backlog-3',
        ...numbered('backlog', 5, 12),
      ]);
      expect((await cardsIn(page, 'ready')).slice(0, 5)).toEqual(READY_WITH_TWO);
      // One drop, one move, carrying both; the place of the grabbed card counts without the other.
      await expect(page.getByTestId('moves')).toHaveText('1');
      await expect(page.getByTestId('last-move')).toHaveText(
        'backlog-4 (+1): backlog 2 to ready 2',
      );
      await expect(page.getByTestId('selected')).toHaveText('0');
    });

    test('a card that is not selected is dragged alone', async ({ page }) => {
      await select(page, 'backlog-2', 'backlog-4');
      await pickUp(page, 'backlog-3');
      await expect(overlay(page).locator('[data-carry-count]')).toHaveCount(0);
      await expect(card(page, 'backlog-2')).toBeVisible();
      await hover(page, 'ready-3', 0.25);
      await page.mouse.up();

      expect((await cardsIn(page, 'ready')).slice(0, 4)).toEqual([
        'ready-1',
        'ready-2',
        'backlog-3',
        'ready-3',
      ]);
      expect(await cardsIn(page, 'backlog')).toEqual([
        'backlog-1',
        'backlog-2',
        ...numbered('backlog', 4, 12),
      ]);
      await expect(page.getByTestId('last-move')).toHaveText('backlog-3: backlog 2 to ready 2');
      // The selection had no part in it and is still there.
      await expect(page.getByTestId('selected')).toHaveText('2');
    });

    test('selected cards that stand apart close up where they are dropped in their own column', async ({
      page,
    }) => {
      await select(page, 'backlog-1', 'backlog-4');
      await pickUp(page, 'backlog-4');
      // Both are out of the way, and one gap stands for them. (Where it first
      // opens is not checked: hiding the card above the pointer pulls the
      // rest of the column up under it.)
      await expect(card(page, 'backlog-1')).toBeHidden();
      await expect(page.locator('[data-drag-gap]')).toHaveCount(1);
      await hover(page, 'backlog-2', 0.2);
      await expectTarget(page, 'backlog', 0);
      await page.mouse.up();
      expect((await cardsIn(page, 'backlog')).slice(0, 5)).toEqual([
        'backlog-1',
        'backlog-4',
        'backlog-2',
        'backlog-3',
        'backlog-5',
      ]);
      await expect(page.getByTestId('last-move')).toHaveText(
        'backlog-4 (+1): backlog 2 to backlog 0',
      );
    });

    test('Escape puts every one of them back', async ({ page }) => {
      await select(page, 'backlog-2', 'backlog-4');
      await pickUp(page, 'backlog-2');
      await hover(page, 'ready-3', 0.25);
      await page.keyboard.press('Escape');
      await page.mouse.up();
      expect(await cardsIn(page, 'backlog')).toEqual(numbered('backlog', 1, 12));
      await expect(card(page, 'backlog-4')).toBeVisible();
      await expect(page.getByTestId('moves')).toHaveText('0');
      await expect(page.getByTestId('selected')).toHaveText('2');
    });
  });

  test('the keyboard carries the selection too', async ({ page }) => {
    await select(page, 'backlog-2', 'backlog-4');
    await card(page, 'backlog-4').focus();
    await page.keyboard.press('Space');
    await expect(announcer(page)).toContainText('and 1 more, position 3 of 12 in Backlog.');
    await expect(page.locator('[data-drag-carried] [data-carry-count]')).toHaveText('2');
    await expect(card(page, 'backlog-2')).toBeHidden();

    await page.keyboard.press('ArrowRight');
    await expectTarget(page, 'ready', 2);
    await expect(announcer(page)).toContainText('and 1 more, position 3 of 32 in Ready.');
    await page.keyboard.press('Space');

    expect((await cardsIn(page, 'ready')).slice(0, 5)).toEqual(READY_WITH_TWO);
    await expect(page.getByTestId('last-move')).toHaveText('backlog-4 (+1): backlog 2 to ready 2');
    await expect(card(page, 'backlog-4')).toBeFocused();
  });

  test('in a sorted view the selection goes to the column, each card to its place in the order', async ({
    page,
  }) => {
    await open(page, '?select=1&sorted=1');
    await select(page, 'backlog-5', 'backlog-8');
    await card(page, 'backlog-8').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('Space');
    await expect(page.getByTestId('last-move')).toHaveText(
      'backlog-8 (+1): backlog 2 to ready, sorted',
    );
    // "Filter by date" and "Import from CSV": each among the cards that say the same.
    const ready = await cardsIn(page, 'ready');
    expect(ready.slice(3, 5)).toEqual(['backlog-5', 'ready-2']);
    expect(ready.slice(8, 10)).toEqual(['backlog-8', 'ready-5']);
  });
});

// --------------------------------------------- a zone: the hidden column

test.describe('a hidden column to drop on', () => {
  const zone = (page: Page): Locator => page.getByTestId('zone');
  const zoneCount = (page: Page): Locator => page.getByTestId('zone-count');

  test.beforeEach(async ({ page }) => {
    // Two short columns: on a wide screen the zone is in sight without scrolling.
    await open(page, '?sizes=3,2&zone=1');
    await expect(zoneCount(page)).toHaveText('0');
  });

  test.describe('mouse', () => {
    test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');

    test('a card dropped on the zone is reported with the zone and no place', async ({ page }) => {
      await pickUp(page, 'backlog-2');
      const on = await pointOn(zone(page));
      await page.mouse.move(on.x, on.y, { steps: 10 });
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'zone');
      await expect(zone(page)).toHaveAttribute('data-over', '');
      // No list shows a gap, and no place is named.
      await expect(page.locator('[data-drag-gap]')).toHaveCount(0);
      await expect(board(page)).not.toHaveAttribute('data-drop-list');
      await page.mouse.up();

      await expect(zoneCount(page)).toHaveText('1');
      expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', 'backlog-3']);
      expect(await cardsIn(page, 'ready')).toEqual(['ready-1', 'ready-2']);
      await expect(page.getByTestId('last-move')).toHaveText('backlog-2: backlog 1 to zone icebox');
      await expect(announcer(page)).toHaveText('Dropped Backlog 2: Dark appearance on Icebox.');
      await expect(zone(page)).not.toHaveAttribute('data-over');
    });

    test('beside the zone is not on it: the nearest column takes the card', async ({ page }) => {
      await pickUp(page, 'backlog-2');
      const box = await zone(page).boundingBox();
      if (!box) throw new Error('the zone is not on the page');
      await page.mouse.move(box.x - 5, box.y + 20, { steps: 10 });
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'index');
      await expect(board(page)).toHaveAttribute('data-drop-list', 'ready');
      await expect(zone(page)).not.toHaveAttribute('data-over');
      // And on it, then off it again.
      await page.mouse.move(box.x + 20, box.y + 20, { steps: 4 });
      await expect(zone(page)).toHaveAttribute('data-over', '');
      await page.mouse.move(box.x - 5, box.y + 20, { steps: 4 });
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'index');
      await page.keyboard.press('Escape');
      await page.mouse.up();
      await expect(zoneCount(page)).toHaveText('0');
    });

    test('without the zone on the page there is nothing to hit there', async ({ page }) => {
      await page.getByRole('switch', { name: 'Hidden column' }).click();
      await expect(zone(page)).toHaveCount(0);
      await pickUp(page, 'backlog-2');
      await page.mouse.move(700, 600, { steps: 10 });
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'index');
      await expectTarget(page, 'ready', 2);
      await page.mouse.up();
      expect(await cardsIn(page, 'ready')).toEqual(['ready-1', 'ready-2', 'backlog-2']);
    });
  });

  test.describe('touch', () => {
    test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');

    test('a finger carries a card to the zone at the far end of the board', async ({ page }) => {
      const size = page.viewportSize();
      if (!size) throw new Error('no viewport');
      await expect(zone(page)).not.toBeInViewport();
      const finger = await Finger.on(page);
      const y = (await pointOn(card(page, 'backlog-2'))).y;
      await finger.pickUp('backlog-2');
      // Held near the right edge until the board has scrolled the zone under the finger.
      await finger.move({ x: size.width - 36, y });
      await expect(zone(page)).toHaveAttribute('data-over', '');
      expect((await scrollOf(board(page))).left).toBeGreaterThan(0);
      await finger.up();

      await expect(zoneCount(page)).toHaveText('1');
      expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', 'backlog-3']);
    });
  });

  test.describe('keyboard', () => {
    test('the zone comes after the last column, and the way back leads to where the card was', async ({
      page,
    }) => {
      await card(page, 'backlog-2').focus();
      await page.keyboard.press('Space');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowDown');
      await expectTarget(page, 'ready', 2);

      await page.keyboard.press('ArrowRight');
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'zone');
      await expect(announcer(page)).toHaveText('Backlog 2: Dark appearance, over Icebox.');
      await expect(zone(page).locator('[data-drag-carried]')).toBeFocused();
      await expect(zone(page)).toBeInViewport();
      await expect(page.locator('[data-drag-carried]')).toHaveCount(1);
      // There is nothing further, and no place within a zone.
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('Home');
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'zone');

      await page.keyboard.press('ArrowLeft');
      await expectTarget(page, 'ready', 2);
      await expect(page.locator('[data-drag-carried]')).toBeFocused();
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('Space');

      await expect(announcer(page)).toHaveText('Dropped Backlog 2: Dark appearance on Icebox.');
      await expect(zoneCount(page)).toHaveText('1');
      expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', 'backlog-3']);
      await expect(page.getByTestId('last-move')).toHaveText('backlog-2: backlog 1 to zone icebox');

      await page.getByRole('button', { name: 'Put them back' }).click();
      await expect(zoneCount(page)).toHaveText('0');
      expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', 'backlog-3', 'backlog-2']);
    });

    test('Escape on the zone puts the card back', async ({ page }) => {
      await card(page, 'backlog-2').focus();
      await page.keyboard.press('Space');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'zone');
      await page.keyboard.press('Escape');
      await expect(zoneCount(page)).toHaveText('0');
      expect(await cardsIn(page, 'backlog')).toEqual(numbered('backlog', 1, 3));
      await expect(card(page, 'backlog-2')).toBeFocused();
    });

    test('without the zone on the page the last column is the end', async ({ page }) => {
      await page.getByRole('switch', { name: 'Hidden column' }).click();
      await card(page, 'backlog-2').focus();
      await page.keyboard.press('Space');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await expect(board(page)).toHaveAttribute('data-drop-kind', 'index');
      await expectTarget(page, 'ready', 1);
      await page.keyboard.press('Escape');
    });
  });
});
