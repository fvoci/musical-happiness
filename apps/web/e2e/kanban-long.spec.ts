/**
 * A kanban column of 400 cards (kanban-long.support.ts): the cards out
 * of sight are not laid out or painted until they come near, and everything
 * a card does still works far down the column. A mouse and a finger drag a
 * card to the bottom edge, where the column scrolls by itself, and drop it
 * where the gap is; several selected cards go together; the keyboard reaches
 * the last card and carries it to the top; a selection stays on cards out of
 * sight; a card opened by its address has the focus, in sight, once it is
 * closed; and a card made at the end of the column is in sight.
 */
import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test';

import { primaryName } from './keyboard.support.ts';
import {
  announcer,
  card,
  cardsBox,
  column,
  dialog,
  expectNoDrag,
  Finger,
  fitPane,
  kanban,
  overlay,
  paneOf,
  pointOn,
  titlesIn,
} from './kanban.support.ts';
import { LONG_COLUMN, longTitle, makeLongColumn, showView } from './kanban-long.support.ts';

// Fewer than the spike's 1,600: enough for the column to be many times higher than the
// view, and quick to make in every project.
const MANY = 400;
/** The three cards of the seed in the long column, and the cards made for it. */
const ALL = MANY + 3;

async function openLong(page: Page, info: TestInfo): Promise<void> {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  await fitPane(page, paneOf(info));
  await showView(page, await makeLongColumn(page, MANY), ALL);
  await expect(column(page, LONG_COLUMN).locator('[data-column-count]')).toContainText(String(ALL));
  expect(errors).toEqual([]);
}

const list = (page: Page): Locator => cardsBox(page, LONG_COLUMN);

const scrollTop = (page: Page): Promise<number> =>
  list(page).evaluate((element) => element.scrollTop);

/** Scrolls the long column, and nothing around it, until the card is in its middle. */
async function centre(page: Page, target: Locator): Promise<void> {
  const box = await target.boundingBox();
  if (!box) throw new Error('the card is not on the page');
  await list(page).evaluate((element, { y, height }) => {
    const within = element.getBoundingClientRect();
    element.scrollTop += y - within.top - (element.clientHeight - height) / 2;
  }, box);
}

/**
 * Picks a card up with the mouse near its top: its middle may lie near the bottom edge of the
 * column, where the column scrolls by itself.
 */
async function pickUp(page: Page, target: Locator): Promise<void> {
  const from = await pointOn(target, 0.2);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 6, from.y - 6, { steps: 3 });
  await expect(overlay(page)).toBeVisible();
}

async function scrollToEnd(page: Page): Promise<void> {
  await list(page).evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
}

test.describe('a column of 400 cards', () => {
  test('has every card in the document and counts them, and those out of sight are skipped', async ({
    page,
  }, info) => {
    await openLong(page, info);
    await expect(list(page).locator('[data-card]')).toHaveCount(ALL);
    // A card far down is in the document with its text, for the page's own search to find.
    await expect(card(page, longTitle(MANY))).toHaveCount(1);
    const skipped = await list(page).evaluate(
      (element) => getComputedStyle(element.lastElementChild ?? element).contentVisibility,
    );
    expect(skipped).toBe('auto');

    await scrollToEnd(page);
    await expect(card(page, longTitle(MANY))).toBeInViewport();
    await expect(card(page, longTitle(MANY))).toBeVisible();
  });

  test('the keyboard reaches the last card, carries it to the top and drops it in sight', async ({
    page,
  }, info) => {
    await openLong(page, info);
    // Back from the button under the column, past the last card's menu: the last card.
    const last = card(page, longTitle(MANY));
    await column(page, LONG_COLUMN).locator('[data-column-new]').focus();
    // The last card's menu may stand between, where it is always there.
    for (
      let stop = 0;
      stop < 2 && !(await last.evaluate((element) => element === document.activeElement));
      stop += 1
    ) {
      await page.keyboard.press('Shift+Tab');
    }
    await expect(last).toBeFocused();
    await expect(last).toBeInViewport();

    await page.keyboard.press('Space');
    await expect(kanban(page)).toHaveAttribute('data-dragging', 'keyboard');
    await expect(announcer(page)).toHaveText(
      `Picked up ${longTitle(MANY)}. ${LONG_COLUMN}, position ${String(ALL)} of ${String(ALL)}.`,
    );
    await page.keyboard.press('Home');
    await expect(page.locator('[data-drag-carried]')).toBeFocused();
    await expect(page.locator('[data-drag-carried]')).toBeInViewport();
    await page.keyboard.press('Space');
    await expect(announcer(page)).toHaveText(
      `Dropped ${longTitle(MANY)} in ${LONG_COLUMN}, position 1 of ${String(ALL)}.`,
    );
    await expect.poll(async () => (await titlesIn(page, LONG_COLUMN))[0]).toBe(longTitle(MANY));
    await expect(last).toBeFocused();
    await expect(last).toBeInViewport();
  });

  test('a selection stays on a card that went out of sight and back', async ({
    page,
    hasTouch,
  }, info) => {
    test.skip(hasTouch, 'a mouse');
    await openLong(page, info);
    const modifier = await primaryName(page);
    const first = card(page, longTitle(1));
    await centre(page, first);
    await first.click({ modifiers: [modifier] });
    await scrollToEnd(page);
    await card(page, longTitle(MANY)).click({ modifiers: [modifier] });
    await expect(page.getByTestId('selection-bar')).toContainText('2 selected');
    await expect(first).not.toBeInViewport();
    await expect(first).toHaveAttribute('data-selected');
    await list(page).evaluate((element) => {
      element.scrollTop = 0;
    });
    // The two go together: dropped before the card above, the one far down comes along. That
    // card in the middle of the column, away from the edges where the column scrolls by itself.
    const before = await titlesIn(page, LONG_COLUMN);
    const above = before[before.indexOf(longTitle(1)) - 1] ?? '';
    await centre(page, card(page, above));
    await expect(first).toBeInViewport();
    await expect(first).toHaveAttribute('data-selected');

    await pickUp(page, first);
    await expect(overlay(page).locator('[data-slot="card-count"]')).toHaveText('2');
    const top = await pointOn(card(page, above), 0.25);
    await page.mouse.move(top.x, top.y, { steps: 6 });
    await expect(list(page).locator('[data-drag-gap]')).toBeVisible();
    await page.mouse.up();
    await expectNoDrag(page);
    await expect
      .poll(async () => {
        const titles = await titlesIn(page, LONG_COLUMN);
        return titles.slice(titles.indexOf(longTitle(1)), titles.indexOf(longTitle(1)) + 3);
      })
      .toEqual([longTitle(1), longTitle(MANY), above]);
  });

  test('a card opened by its address has the focus, in sight, once it is closed', async ({
    page,
  }, info) => {
    await openLong(page, info);
    const far = card(page, longTitle(300));
    const cardId = (await far.getAttribute('data-card')) ?? '';
    await page.evaluate((id) => {
      history.pushState(null, '', `${location.pathname}/${encodeURIComponent(id)}`);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, cardId);
    await expect(dialog(page)).toContainText(longTitle(300));
    await page.keyboard.press('Escape');
    await expect(dialog(page)).toBeHidden();
    await expect(far).toBeFocused();
    await expect(far).toBeInViewport();
  });

  test('a card made at the end of the column is in sight once it is closed', async ({
    page,
  }, info) => {
    await openLong(page, info);
    await column(page, LONG_COLUMN).locator('[data-column-new]').click();
    await expect(dialog(page)).toBeVisible();
    await page.goBack();
    await expect(dialog(page)).toBeHidden();
    await expect(list(page).locator('[data-card]')).toHaveCount(ALL + 1);
    const made = list(page).locator('[data-card]').last();
    await expect(made).toBeFocused();
    await expect(made).toBeInViewport();
  });
});

test.describe('dragging down a column of 400 cards', () => {
  /**
   * Holds the card at the bottom edge of the column until the column has
   * scrolled by itself past cards never drawn, then drops it where the gap
   * is: next to the card the gap stands before.
   */
  async function dragToEdge(
    page: Page,
    move: (to: { x: number; y: number }) => Promise<void>,
    drop: () => Promise<void>,
  ): Promise<void> {
    const box = await list(page).boundingBox();
    if (!box) throw new Error('no column');
    const edge = { x: box.x + box.width / 2, y: box.y + box.height - 6 };
    await move(edge);
    // A column and a half: past what was in sight, and drawn around it, when the drag began.
    await expect
      .poll(async () => {
        await move(edge);
        return scrollTop(page);
      })
      .toBeGreaterThan(box.height * 1.5);
    // Away from the edge, the gap stands before a card in sight.
    await move({ x: edge.x, y: box.y + box.height / 2 });
    const gap = list(page).locator('[data-drag-gap]');
    await expect(gap).toBeInViewport();
    const after = await gap.evaluate(
      (element) =>
        element.nextElementSibling?.querySelector('[data-slot="card-title-text"]')?.textContent ??
        '',
    );
    expect(after).toMatch(/^Long \d+$/);
    await drop();
    await expectNoDrag(page);
    const titles = await titlesIn(page, LONG_COLUMN);
    expect(titles.indexOf(after) - titles.indexOf(longTitle(1))).toBe(1);
    expect(titles.length).toBe(ALL);
  }

  test('a mouse', async ({ page, hasTouch }, info) => {
    test.skip(hasTouch, 'a mouse');
    await openLong(page, info);
    await centre(page, card(page, longTitle(1)));
    await pickUp(page, card(page, longTitle(1)));
    await dragToEdge(
      page,
      (to) => page.mouse.move(to.x, to.y, { steps: 2 }),
      () => page.mouse.up(),
    );
  });

  test('a finger', async ({ page, hasTouch }, info) => {
    test.skip(!hasTouch, 'a finger');
    await openLong(page, info);
    const finger = await Finger.on(page);
    await centre(page, card(page, longTitle(1)));
    await finger.rest(card(page, longTitle(1)), 0.2);
    await dragToEdge(
      page,
      (to) => finger.move(to, 2),
      () => finger.up(),
    );
  });
});
