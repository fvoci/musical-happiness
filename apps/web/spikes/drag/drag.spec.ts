/**
 * The drag spike in a browser: a real mouse, a real finger (touch events sent
 * through the DevTools protocol, which run through the browser's own gesture
 * handling) and the keyboard.
 *
 * Every test ends by reading the order of the cards from the page.
 *
 * A target is always measured where it lies *now*: the gap that follows the
 * pointer moves the cards below it, so a position measured before the drag
 * began is no longer where the card is.
 */
import { expect, test, type CDPSession, type Locator, type Page } from '@playwright/test';

const PAGE = '/spikes/drag/';

interface Point {
  x: number;
  y: number;
}

const card = (page: Page, id: string): Locator => page.locator(`[data-card="${id}"]`);
const list = (page: Page, column: string): Locator => page.locator(`[data-drag-list="${column}"]`);
const board = (page: Page): Locator => page.getByTestId('board');
const overlay = (page: Page): Locator => page.locator('[data-drag-overlay]');

/** The ids of the cards of a column, top to bottom. */
async function cardsIn(page: Page, column: string): Promise<string[]> {
  return page
    .locator(`[data-column="${column}"] [data-card]`)
    .evaluateAll((cards) => cards.map((element) => element.getAttribute('data-card') ?? ''));
}

function numbered(column: string, from: number, to: number): string[] {
  return Array.from({ length: to - from + 1 }, (_, index) => `${column}-${from + index}`);
}

/** A point on an element as it lies now: in the middle across, `down` of the way from its top. */
async function pointOn(locator: Locator, down = 0.5): Promise<Point> {
  const box = await locator.boundingBox();
  if (!box) throw new Error('the element is not on the page');
  return { x: box.x + box.width / 2, y: box.y + box.height * down };
}

async function nextFrame(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        requestAnimationFrame(resolve);
      }),
  );
}

async function scrollOf(locator: Locator): Promise<{ left: number; top: number; atEnd: boolean }> {
  return locator.evaluate((element) => ({
    left: element.scrollLeft,
    top: element.scrollTop,
    atEnd:
      element.scrollLeft + element.clientWidth >= element.scrollWidth - 1 &&
      element.scrollTop + element.clientHeight >= element.scrollHeight - 1,
  }));
}

/** Where the dragged card would land, as the page reports it. */
async function expectTarget(page: Page, column: string, index: number): Promise<void> {
  await expect(board(page)).toHaveAttribute('data-drop-list', column);
  await expect(board(page)).toHaveAttribute('data-drop-index', String(index));
}

async function expectNothingMoved(page: Page): Promise<void> {
  await expect(board(page)).not.toHaveAttribute('data-dragging');
  await expect(overlay(page)).toHaveCount(0);
  await expect(page.locator('[data-drag-gap]')).toHaveCount(0);
  await expect(page.getByTestId('moves')).toHaveText('0');
  expect(await cardsIn(page, 'backlog')).toEqual(numbered('backlog', 1, 12));
  expect(await cardsIn(page, 'ready')).toEqual(numbered('ready', 1, 30));
}

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(PAGE);
  await expect(card(page, 'backlog-1')).toBeVisible();
  expect(errors).toEqual([]);
});

// ------------------------------------------------------------------ mouse

test.describe('mouse', () => {
  test.skip(({ hasTouch }) => hasTouch, 'the mouse is tested in the desktop project');

  /** Presses on a card and moves far enough for the drag to begin. */
  async function pickUp(page: Page, id: string): Promise<void> {
    const from = await pointOn(card(page, id));
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(from.x + 6, from.y + 6, { steps: 3 });
    await expect(overlay(page)).toBeVisible();
  }

  /** Moves onto a card as it lies now. Twice: the first move may open a gap that shifts it. */
  async function hover(page: Page, id: string, down: number): Promise<void> {
    for (let pass = 0; pass < 2; pass += 1) {
      const to = await pointOn(card(page, id), down);
      await page.mouse.move(to.x, to.y, { steps: 8 });
      await nextFrame(page);
    }
  }

  test('moves a card to a chosen place in another column', async ({ page }) => {
    await pickUp(page, 'backlog-2');
    // The card under the pointer is its own copy; the original is out of the way.
    await expect(card(page, 'backlog-2')).toBeHidden();
    await expect(page.locator('[data-drag-gap]')).toHaveCount(1);

    await hover(page, 'ready-3', 0.25);
    await expectTarget(page, 'ready', 2);
    await page.mouse.up();

    expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', ...numbered('backlog', 3, 12)]);
    expect(await cardsIn(page, 'ready')).toEqual([
      'ready-1',
      'ready-2',
      'backlog-2',
      ...numbered('ready', 3, 30),
    ]);
    await expect(page.getByTestId('last-move')).toHaveText('backlog-2: backlog 1 to ready 2');
    await expect(card(page, 'backlog-2')).toBeVisible();
    await expect(overlay(page)).toHaveCount(0);
    // The button going up at the end of a drag is not a click on the card.
    await expect(page.getByTestId('opened')).toHaveText('none');
  });

  test('the lower half of a card is the place after it', async ({ page }) => {
    await pickUp(page, 'backlog-2');
    await hover(page, 'ready-3', 0.8);
    await expectTarget(page, 'ready', 3);
    await page.mouse.up();
    expect((await cardsIn(page, 'ready')).slice(0, 5)).toEqual([
      'ready-1',
      'ready-2',
      'ready-3',
      'backlog-2',
      'ready-4',
    ]);
  });

  test('reorders a card within its column, down and up', async ({ page }) => {
    await pickUp(page, 'backlog-1');
    await hover(page, 'backlog-4', 0.8);
    await expectTarget(page, 'backlog', 3);
    await page.mouse.up();
    expect(await cardsIn(page, 'backlog')).toEqual([
      'backlog-2',
      'backlog-3',
      'backlog-4',
      'backlog-1',
      ...numbered('backlog', 5, 12),
    ]);

    await pickUp(page, 'backlog-5');
    await hover(page, 'backlog-2', 0.2);
    await expectTarget(page, 'backlog', 0);
    await page.mouse.up();
    expect(await cardsIn(page, 'backlog')).toEqual([
      'backlog-5',
      'backlog-2',
      'backlog-3',
      'backlog-4',
      'backlog-1',
      ...numbered('backlog', 6, 12),
    ]);
    await expect(page.getByTestId('moves')).toHaveText('2');
  });

  test('dropping a card where it was changes nothing', async ({ page }) => {
    await pickUp(page, 'backlog-3');
    await page.mouse.up();
    await expectNothingMoved(page);
  });

  test('a drag to a column and a place that are both out of sight scrolls to them', async ({
    page,
  }) => {
    const size = page.viewportSize();
    if (!size) throw new Error('no viewport');
    await expect(list(page, 'archive')).not.toBeInViewport();

    await pickUp(page, 'backlog-1');
    // Held at the right edge, without moving: the board scrolls under the pointer.
    const middle = (await pointOn(board(page))).y;
    await page.mouse.move(size.width - 6, middle, { steps: 12 });
    await expect.poll(async () => (await scrollOf(board(page))).atEnd).toBe(true);
    await expect(board(page)).toHaveAttribute('data-drop-list', 'archive');
    await expect(list(page, 'archive')).toBeInViewport();

    // Held at the foot of the last column: the column scrolls, and the target
    // goes down the list although the pointer stays where it is.
    const foot = await pointOn(list(page, 'archive'), 1);
    await page.mouse.move(foot.x, foot.y - 8, { steps: 12 });
    await nextFrame(page);
    const early = await dropIndex(page);
    await expect.poll(async () => (await scrollOf(list(page, 'archive'))).atEnd).toBe(true);
    await expect.poll(() => dropIndex(page)).toBeGreaterThan(early);
    await expect(board(page)).toHaveAttribute('data-drop-list', 'archive');

    // After the scrolling the cards are hit where they now lie.
    await hover(page, 'archive-22', 0.25);
    await expectTarget(page, 'archive', 21);
    await page.mouse.up();

    expect(await cardsIn(page, 'backlog')).toEqual(numbered('backlog', 2, 12));
    expect(await cardsIn(page, 'archive')).toEqual([
      ...numbered('archive', 1, 21),
      'backlog-1',
      ...numbered('archive', 22, 24),
    ]);
    // A drop keeps the board where the drag scrolled it to.
    expect((await scrollOf(board(page))).left).toBeGreaterThan(0);
  });

  test('Escape cancels: order, scrolling and the pressed button are as if nothing happened', async ({
    page,
  }) => {
    const size = page.viewportSize();
    if (!size) throw new Error('no viewport');
    await pickUp(page, 'backlog-2');
    await hover(page, 'ready-5', 0.5);
    await expect(board(page)).toHaveAttribute('data-drop-list', 'ready');
    const middle = (await pointOn(board(page))).y;
    await page.mouse.move(size.width - 6, middle, { steps: 12 });
    await expect.poll(async () => (await scrollOf(board(page))).left).toBeGreaterThan(200);

    await page.keyboard.press('Escape');
    await expectNothingMoved(page);
    expect((await scrollOf(board(page))).left).toBe(0);

    // The button is still down. Letting go, even on the card itself, does nothing.
    const back = await pointOn(card(page, 'backlog-2'));
    await page.mouse.move(back.x, back.y, { steps: 5 });
    await page.mouse.up();
    await expectNothingMoved(page);
    await expect(page.getByTestId('opened')).toHaveText('none');

    // And the next press is an ordinary one again.
    await card(page, 'backlog-2').click();
    await expect(page.getByTestId('opened')).toHaveText('backlog-2');
  });

  test('losing the pointer capture cancels', async ({ page }) => {
    await page.evaluate(() => {
      window.addEventListener(
        'pointerdown',
        (event) => {
          document.documentElement.dataset.pointer = String(event.pointerId);
        },
        true,
      );
    });
    await pickUp(page, 'backlog-2');
    await hover(page, 'ready-3', 0.25);
    await expectTarget(page, 'ready', 2);

    await board(page).evaluate((element) => {
      element.releasePointerCapture(Number(document.documentElement.dataset.pointer));
    });
    // The browser reports the loss with the next event of the pointer.
    await page.mouse.move(300, 300);
    await expectNothingMoved(page);
    await page.mouse.up();
    await expectNothingMoved(page);
  });

  test('a click without a drag still opens the card', async ({ page }) => {
    await card(page, 'ready-2').click();
    await expect(page.getByTestId('opened')).toHaveText('ready-2');
    await expectNothingMoved(page);
  });

  test('a drop into an empty column', async ({ page }) => {
    await page.goto(`${PAGE}?sizes=3,0,2`);
    await pickUp(page, 'backlog-2');
    const empty = await pointOn(list(page, 'ready'));
    await page.mouse.move(empty.x, empty.y + 200, { steps: 8 });
    await expectTarget(page, 'ready', 0);
    await page.mouse.up();
    expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', 'backlog-3']);
    expect(await cardsIn(page, 'ready')).toEqual(['backlog-2']);
  });
});

async function dropIndex(page: Page): Promise<number> {
  return Number(await board(page).getAttribute('data-drop-index'));
}

// ------------------------------------------------------------------ touch

/** One finger, moved by touch events sent to the browser as a touch screen would send them. */
class Finger {
  readonly #client: CDPSession;
  readonly #page: Page;
  #at: Point = { x: 0, y: 0 };

  constructor(client: CDPSession, page: Page) {
    this.#client = client;
    this.#page = page;
  }

  static async on(page: Page): Promise<Finger> {
    return new Finger(await page.context().newCDPSession(page), page);
  }

  async #send(type: 'touchStart' | 'touchMove', at: Point): Promise<void> {
    this.#at = at;
    await this.#client.send('Input.dispatchTouchEvent', {
      type,
      touchPoints: [{ x: Math.round(at.x), y: Math.round(at.y), id: 1 }],
    });
  }

  async down(at: Point): Promise<void> {
    await this.#send('touchStart', at);
  }

  /** Moves in steps, a frame apart, as a finger does. */
  async move(to: Point, steps = 8): Promise<void> {
    const from = this.#at;
    for (let step = 1; step <= steps; step += 1) {
      await this.#send('touchMove', {
        x: from.x + ((to.x - from.x) * step) / steps,
        y: from.y + ((to.y - from.y) * step) / steps,
      });
      await nextFrame(this.#page);
    }
  }

  async up(): Promise<void> {
    await this.#client.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  }

  /** What the system does when it takes the touch away from the page. */
  async cancel(): Promise<void> {
    await this.#client.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] });
  }

  /** Rests on a card until the drag begins. */
  async pickUp(id: string): Promise<void> {
    await this.down(await pointOn(card(this.#page, id)));
    await expect(overlay(this.#page)).toBeVisible();
  }

  /**
   * Moves onto a card as it lies now. Twice, as with the mouse. Across, the
   * finger stays out of the strips at the sides of the screen in which the
   * board would scroll.
   */
  async hover(id: string, down: number): Promise<void> {
    const width = this.#page.viewportSize()?.width ?? 0;
    for (let pass = 0; pass < 2; pass += 1) {
      const to = await pointOn(card(this.#page, id), down);
      await this.move({ x: Math.min(Math.max(to.x, 70), width - 70), y: to.y });
    }
  }
}

test.describe('touch', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'touch is tested in the touch project');

  test('a swipe over the cards scrolls the column and drags nothing', async ({ page }) => {
    const finger = await Finger.on(page);
    const start = await pointOn(card(page, 'backlog-4'));
    await finger.down(start);
    await finger.move({ x: start.x, y: start.y - 260 }, 10);
    await finger.up();

    await expect.poll(async () => (await scrollOf(list(page, 'backlog'))).top).toBeGreaterThan(100);
    await expectNothingMoved(page);
  });

  test('a swipe sideways over the cards scrolls the board and drags nothing', async ({ page }) => {
    const finger = await Finger.on(page);
    const start = await pointOn(card(page, 'backlog-4'));
    await finger.down({ x: start.x + 80, y: start.y });
    await finger.move({ x: start.x - 180, y: start.y }, 10);
    await finger.up();

    await expect.poll(async () => (await scrollOf(board(page))).left).toBeGreaterThan(100);
    await expectNothingMoved(page);
  });

  test('a finger that moves off before the wait is over does not drag', async ({ page }) => {
    // Sideways on a board that is already at its left end: the browser has nothing to scroll.
    const finger = await Finger.on(page);
    const start = await pointOn(card(page, 'backlog-2'));
    await finger.down(start);
    await finger.move({ x: start.x + 60, y: start.y }, 6);
    await page.waitForTimeout(500);
    await expect(overlay(page)).toHaveCount(0);
    await finger.up();
    await expectNothingMoved(page);
  });

  test('a long press picks a card up; it is reordered within its column', async ({ page }) => {
    const finger = await Finger.on(page);
    await finger.pickUp('backlog-1');
    await expect(card(page, 'backlog-1')).toBeHidden();

    await finger.hover('backlog-3', 0.8);
    await expectTarget(page, 'backlog', 2);
    // The finger moved a long way down the column and the column did not scroll with it.
    expect((await scrollOf(list(page, 'backlog'))).top).toBe(0);
    await finger.up();

    expect(await cardsIn(page, 'backlog')).toEqual([
      'backlog-2',
      'backlog-3',
      'backlog-1',
      ...numbered('backlog', 4, 12),
    ]);
    await expect(page.getByTestId('opened')).toHaveText('none');
  });

  test('moves a card to the next column, which the board scrolls into view', async ({ page }) => {
    const size = page.viewportSize();
    if (!size) throw new Error('no viewport');
    const finger = await Finger.on(page);
    const y = (await pointOn(card(page, 'backlog-2'))).y;
    await finger.pickUp('backlog-2');

    // Held near the right edge, where the board scrolls slowly, until the
    // next column has come in.
    await finger.move({ x: size.width - 36, y });
    await expect
      .poll(async () => (await list(page, 'ready').boundingBox())?.x ?? Infinity)
      .toBeLessThan(90);
    // Back to the middle, where nothing scrolls, and onto the card as it now lies.
    await finger.move({ x: size.width / 2, y });
    const settled = (await scrollOf(board(page))).left;
    expect(settled).toBeGreaterThan(0);
    await finger.hover('ready-3', 0.25);
    await expectTarget(page, 'ready', 2);
    expect((await scrollOf(board(page))).left).toBe(settled);
    await finger.up();

    expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', ...numbered('backlog', 3, 12)]);
    expect(await cardsIn(page, 'ready')).toEqual([
      'ready-1',
      'ready-2',
      'backlog-2',
      ...numbered('ready', 3, 30),
    ]);
  });

  test('a drag to the end of a long column scrolls the column', async ({ page }) => {
    const finger = await Finger.on(page);
    await finger.pickUp('backlog-1');
    const x = (await pointOn(card(page, 'backlog-2'))).x;
    const foot = await pointOn(list(page, 'backlog'), 1);
    await finger.move({ x, y: foot.y - 8 }, 12);
    await nextFrame(page);
    const early = await dropIndex(page);

    await expect.poll(async () => (await scrollOf(list(page, 'backlog'))).atEnd).toBe(true);
    // The finger rested; the target went down the list with the scrolling.
    await expect.poll(() => dropIndex(page)).toBeGreaterThan(early);
    await finger.hover('backlog-12', 0.8);
    await expectTarget(page, 'backlog', 11);
    await finger.up();

    expect(await cardsIn(page, 'backlog')).toEqual([...numbered('backlog', 2, 12), 'backlog-1']);
  });

  test('a touch the system takes away cancels', async ({ page }) => {
    const finger = await Finger.on(page);
    await finger.pickUp('backlog-1');
    await finger.hover('backlog-3', 0.8);
    await expectTarget(page, 'backlog', 2);
    await finger.cancel();
    await expectNothingMoved(page);
  });
});

// --------------------------------------------------------------- keyboard

test.describe('keyboard', () => {
  const announcer = (page: Page): Locator => page.getByTestId('announcer');

  test('picks a card up, moves it within and between columns and drops it', async ({ page }) => {
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Space');
    await expect(announcer(page)).toHaveText(
      'Picked up Backlog 2: Dark appearance, position 2 of 12 in Backlog.',
    );
    await expect(board(page)).toHaveAttribute('data-dragging', 'keyboard');
    await expect(page.locator('[data-drag-carried]')).toBeFocused();

    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await expect(announcer(page)).toHaveText(
      'Backlog 2: Dark appearance, position 4 of 12 in Backlog.',
    );
    await page.keyboard.press('ArrowRight');
    await expect(announcer(page)).toHaveText(
      'Backlog 2: Dark appearance, position 4 of 31 in Ready.',
    );
    await page.keyboard.press('ArrowUp');
    await expectTarget(page, 'ready', 2);
    await expect(page.locator('[data-drag-carried]')).toBeFocused();
    // Nothing is moved before the drop.
    await expect(page.getByTestId('moves')).toHaveText('0');

    await page.keyboard.press('Space');
    await expect(announcer(page)).toHaveText(
      'Dropped Backlog 2: Dark appearance, position 3 of 31 in Ready.',
    );
    expect(await cardsIn(page, 'backlog')).toEqual(['backlog-1', ...numbered('backlog', 3, 12)]);
    expect(await cardsIn(page, 'ready')).toEqual([
      'ready-1',
      'ready-2',
      'backlog-2',
      ...numbered('ready', 3, 30),
    ]);
    await expect(card(page, 'backlog-2')).toBeFocused();
    await expect(board(page)).not.toHaveAttribute('data-dragging');
  });

  test('stops at the ends, and Home and End go there', async ({ page }) => {
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowLeft');
    await expectTarget(page, 'backlog', 1);
    await page.keyboard.press('Home');
    await page.keyboard.press('ArrowUp');
    await expectTarget(page, 'backlog', 0);
    await page.keyboard.press('End');
    await page.keyboard.press('ArrowDown');
    await expectTarget(page, 'backlog', 11);
    await page.keyboard.press('Enter');

    expect(await cardsIn(page, 'backlog')).toEqual([
      'backlog-1',
      ...numbered('backlog', 3, 12),
      'backlog-2',
    ]);
    // Enter dropped the card and did not also open it.
    await expect(page.getByTestId('opened')).toHaveText('none');
  });

  test('carries a card to a column that is out of sight, and the board follows', async ({
    page,
  }) => {
    await expect(list(page, 'archive')).not.toBeInViewport();
    await card(page, 'backlog-1').focus();
    await page.keyboard.press('Space');
    for (let step = 0; step < 5; step += 1) await page.keyboard.press('ArrowRight');
    await page.keyboard.press('End');
    await expectTarget(page, 'archive', 24);
    await expect(page.locator('[data-drag-carried]')).toBeInViewport();
    await page.keyboard.press('Space');

    expect(await cardsIn(page, 'archive')).toEqual([...numbered('archive', 1, 24), 'backlog-1']);
    await expect(card(page, 'backlog-1')).toBeFocused();
    await expect(card(page, 'backlog-1')).toBeInViewport();
  });

  test('Escape puts the card back', async ({ page }) => {
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowDown');
    await expectTarget(page, 'ready', 2);

    await page.keyboard.press('Escape');
    await expect(announcer(page)).toHaveText(
      'Cancelled. Backlog 2: Dark appearance is back at position 2 of 12 in Backlog.',
    );
    await expectNothingMoved(page);
    await expect(page.locator('[data-drag-carried]')).toHaveCount(0);
    await expect(card(page, 'backlog-2')).toBeFocused();
  });

  test('Tab does not leave a carried card behind', async ({ page }) => {
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('Tab');
    await expect(page.locator('[data-drag-carried]')).toBeFocused();
    await page.keyboard.press('Escape');
    await expectNothingMoved(page);
  });

  test('reaching for the pointer gives a carried card up', async ({ page }) => {
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowRight');
    await expectTarget(page, 'ready', 1);
    await page.getByRole('heading', { name: 'Drag spike' }).click();
    await expectNothingMoved(page);
    await expect(page.locator('[data-drag-carried]')).toHaveCount(0);
  });

  test('Enter on a card that is not carried opens it', async ({ page }) => {
    await card(page, 'backlog-2').focus();
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('opened')).toHaveText('backlog-2');
    await expectNothingMoved(page);
  });
});
