/**
 * What the browser tests of the drag spike share: where things are on the
 * page, a mouse that picks up, and a finger.
 *
 * A target is always measured where it lies *now*: the gap that follows the
 * pointer moves the cards below it, so a position measured before the drag
 * began is no longer where the card is.
 */
import { expect, type CDPSession, type Locator, type Page } from '@playwright/test';

export const PAGE = '/spikes/drag/';

export interface Point {
  x: number;
  y: number;
}

export const card = (page: Page, id: string): Locator => page.locator(`[data-card="${id}"]`);
export const list = (page: Page, column: string): Locator =>
  page.locator(`[data-drag-list="${column}"]`);
export const heading = (page: Page, column: string): Locator =>
  page.locator(`[data-column-heading="${column}"]`);
export const board = (page: Page): Locator => page.getByTestId('board');
export const overlay = (page: Page): Locator => page.locator('[data-drag-overlay]');
export const announcer = (page: Page): Locator => page.getByTestId('announcer');
export const notice = (page: Page): Locator => page.getByTestId('notice');

/** The ids of the cards of a column, top to bottom. */
export async function cardsIn(page: Page, column: string): Promise<string[]> {
  return page
    .locator(`[data-column="${column}"] [data-card]`)
    .evaluateAll((cards) => cards.map((element) => element.getAttribute('data-card') ?? ''));
}

/** The ids of the columns, in the order they stand in. */
export async function columnsOf(page: Page): Promise<string[]> {
  return page
    .locator('[data-column]')
    .evaluateAll((columns) => columns.map((element) => element.getAttribute('data-column') ?? ''));
}

export function numbered(column: string, from: number, to: number): string[] {
  return Array.from({ length: to - from + 1 }, (_, index) => `${column}-${from + index}`);
}

/** A point on an element as it lies now: in the middle across, `down` of the way from its top. */
export async function pointOn(locator: Locator, down = 0.5): Promise<Point> {
  const box = await locator.boundingBox();
  if (!box) throw new Error('the element is not on the page');
  return { x: box.x + box.width / 2, y: box.y + box.height * down };
}

export async function nextFrame(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        requestAnimationFrame(resolve);
      }),
  );
}

export async function scrollOf(
  locator: Locator,
): Promise<{ left: number; top: number; atEnd: boolean }> {
  return locator.evaluate((element) => ({
    left: element.scrollLeft,
    top: element.scrollTop,
    atEnd:
      element.scrollLeft + element.clientWidth >= element.scrollWidth - 1 &&
      element.scrollTop + element.clientHeight >= element.scrollHeight - 1,
  }));
}

/** Where the dragged card would land, as the page reports it. */
export async function expectTarget(page: Page, column: string, index: number): Promise<void> {
  await expect(board(page)).toHaveAttribute('data-drop-list', column);
  await expect(board(page)).toHaveAttribute('data-drop-index', String(index));
}

export async function dropIndex(page: Page): Promise<number> {
  return Number(await board(page).getAttribute('data-drop-index'));
}

/** No drag is going on and nothing is left of one. */
export async function expectNoDrag(page: Page): Promise<void> {
  await expect(board(page)).not.toHaveAttribute('data-dragging');
  await expect(overlay(page)).toHaveCount(0);
  await expect(page.locator('[data-drag-gap]')).toHaveCount(0);
  await expect(page.locator('[data-column-gap]')).toHaveCount(0);
}

/** No drag is going on and the default board is as it was loaded. */
export async function expectNothingMoved(page: Page): Promise<void> {
  await expect(board(page)).not.toHaveAttribute('data-dragging');
  await expect(overlay(page)).toHaveCount(0);
  await expect(page.locator('[data-drag-gap]')).toHaveCount(0);
  await expect(page.getByTestId('moves')).toHaveText('0');
  expect(await cardsIn(page, 'backlog')).toEqual(numbered('backlog', 1, 12));
  expect(await cardsIn(page, 'ready')).toEqual(numbered('ready', 1, 30));
}

/** Opens the page, by default the plain board, and fails on any error the page throws while loading. */
export async function open(page: Page, query = ''): Promise<void> {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`${PAGE}${query}`);
  await expect(page.locator('[data-column]').first()).toBeVisible();
  expect(errors).toEqual([]);
}

// ------------------------------------------------------------------ mouse

/** Presses on an element and moves far enough for the drag to begin. */
export async function grab(page: Page, target: Locator): Promise<void> {
  const from = await pointOn(target);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 6, from.y + 6, { steps: 3 });
  await expect(overlay(page)).toBeVisible();
}

/** Presses on a card and moves far enough for the drag to begin. */
export async function pickUp(page: Page, id: string): Promise<void> {
  await grab(page, card(page, id));
}

/** Moves onto a card as it lies now. Twice: the first move may open a gap that shifts it. */
export async function hover(page: Page, id: string, down: number): Promise<void> {
  for (let pass = 0; pass < 2; pass += 1) {
    const to = await pointOn(card(page, id), down);
    await page.mouse.move(to.x, to.y, { steps: 8 });
    await nextFrame(page);
  }
}

// ------------------------------------------------------------------ touch

/** One finger, moved by touch events sent to the browser as a touch screen would send them. */
export class Finger {
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

  /** Rests on an element until the drag begins. */
  async rest(target: Locator): Promise<void> {
    await this.down(await pointOn(target));
    await expect(overlay(this.#page)).toBeVisible();
  }

  /** Rests on a card until the drag begins. */
  async pickUp(id: string): Promise<void> {
    await this.rest(card(this.#page, id));
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
