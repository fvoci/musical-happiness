/**
 * The calendar mock-ups in a browser: every screen and proposal comes up,
 * nothing spills sideways, what a finger has to hit is large enough, the
 * arrows change the month, and each proposal is the thing it claims to be.
 *
 * The `touch` project looks at the phone (360×740, a finger) and at the
 * sizes in between; the `desktop` project at the desktop (1280×800, a mouse).
 */
import { expect, test, type Page } from '@playwright/test';

import { SCREENS, SIZES, type Variant } from './params.ts';
import { DESKTOP, NOW, PAGE, PHONE, show } from './support.ts';

/** `--spacing-touch`, in pixels at the default text size. */
const TOUCH = 44;

const ALL = SCREENS.flatMap((screen) =>
  (Object.keys(screen.variants) as Variant[]).map((variant) => ({ screen: screen.id, variant })),
);

let errors: string[] = [];

test.beforeEach(async ({ page, hasTouch }) => {
  errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text());
  });
  await page.setViewportSize(hasTouch ? PHONE : DESKTOP);
  await page.clock.setFixedTime(NOW);
});

/** How far the document can be scrolled sideways. */
async function spill(page: Page): Promise<number> {
  return page.evaluate(() => {
    const root = document.documentElement;
    return Math.max(root.scrollWidth, document.body.scrollWidth) - root.clientWidth;
  });
}

/**
 * The buttons that a finger would miss: narrower or lower than the smallest
 * target. A switch inside its label is as large as the label, which passes a
 * tap on to it. The list of boards is left out: it is the sidebar of the
 * mobile mock-ups as it is, and what is small in it is noted there. A
 * hundredth of a pixel is forgiven: a popover is placed at a fraction of one.
 */
async function smallTargets(page: Page): Promise<string[]> {
  return page.locator('button').evaluateAll(
    (buttons, least) =>
      buttons.flatMap((button) => {
        if (button.closest('[data-testid="sidebar"]')) return [];
        const box = (button.closest('label') ?? button).getBoundingClientRect();
        if (box.width === 0 || box.height === 0) return [];
        if (box.width >= least && box.height >= least) return [];
        const name = button.getAttribute('aria-label') ?? button.textContent.trim();
        return [`${name} ${box.width.toFixed(2)}x${box.height.toFixed(2)}`];
      }),
    TOUCH - 0.01,
  );
}

const period = (page: Page) => page.getByTestId('period');
const box = (page: Page) => page.getByTestId('calendar-box');
const weekdays = (page: Page) => page.locator('[data-weekday]').allTextContents();
/** The day's button: its name says the date and how many cards are on it. */
const day = (page: Page, date: string) =>
  page.locator(`button[data-day="${date}"], [data-day="${date}"] > button`).first();

test.describe('every screen and proposal', () => {
  for (const { screen, variant } of ALL) {
    test(`${screen} ${variant} comes up without an error and without spilling sideways`, async ({
      page,
    }) => {
      await show(page, screen, variant);
      expect(errors).toEqual([]);
      expect(await spill(page)).toBe(0);
      expect(await page.locator('html').getAttribute('lang')).toBe('ko');
      // A month by its name; a week by its days.
      await expect(period(page)).toHaveText(screen === 'week' ? '11월 8일~14일' : '2026년 11월');
    });
  }

  test('the chooser lists every screen', async ({ page }) => {
    await page.goto(PAGE);
    for (const screen of SCREENS) {
      await expect(page.getByRole('button', { name: screen.title, exact: true })).toBeVisible();
    }
    expect(errors).toEqual([]);
    expect(await spill(page)).toBe(0);
  });
});

test.describe('the view of one week', () => {
  test('names the days it shows, and the arrows step by a week', async ({ page }) => {
    await show(page, 'week', 'a');
    await expect(period(page)).toHaveText('11월 8일~14일');
    await page.getByTestId('next').click();
    await expect(period(page)).toHaveText('11월 15일~21일');
    await expect(box(page)).toHaveAttribute('data-selected', '2026-11-17');
    expect(errors).toEqual([]);
  });
});

test.describe('on a phone', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'the narrow layout, with a finger');

  test('the month is a small grid over the cards of the picked day', async ({ page }) => {
    await show(page, 'month', 'a');
    await expect(page.getByTestId('mini-month')).toBeVisible();
    await expect(page.getByTestId('month-grid')).toHaveCount(0);
    expect(await weekdays(page)).toEqual(['일', '월', '화', '수', '목', '금', '토']);
    await expect(page.locator('[data-week]')).toHaveCount(5);
    await expect(page.locator('[aria-current="date"]')).toHaveAttribute('data-day', '2026-11-10');

    const list = page.getByTestId('day-list');
    await expect(list).toHaveAttribute('data-date', '2026-11-10');
    await expect(list.locator('[data-card]')).toHaveCount(2);

    await day(page, '2026-11-12').tap();
    await expect(list).toHaveAttribute('data-date', '2026-11-12');
    await expect(list.locator('[data-card]')).toHaveCount(7);
    await expect(day(page, '2026-11-12')).toHaveAttribute('aria-pressed', 'true');
  });

  test('the second proposal is a list of days under a strip of one week', async ({ page }) => {
    await show(page, 'month', 'b');
    await expect(page.getByTestId('mini-month')).toHaveCount(0);
    await expect(page.getByTestId('week-strip').getByRole('button')).toHaveCount(7);
    const agenda = page.getByTestId('agenda');
    // The week of today from its first day with a card; a card of several days under each of them.
    await expect(agenda.locator('[data-agenda-day]').first()).toHaveAttribute(
      'data-agenda-day',
      '2026-11-09',
    );
    await expect(agenda.locator('[data-agenda-day="2026-11-10"] [data-card="c1"]')).toBeVisible();
    // Every day has its own "+", so there is no round button over the list.
    await expect(page.getByTestId('new-card-fab')).toHaveCount(0);
  });

  test('the arrows change the month, and "today" comes back', async ({ page }) => {
    await show(page, 'month', 'a');
    await page.getByTestId('next').tap();
    await expect(period(page)).toHaveText('2026년 12월');
    await expect(box(page)).toHaveAttribute('data-month', '2026-12');
    await expect(box(page)).toHaveAttribute('data-selected', '2026-12-01');
    await page.getByTestId('previous').tap();
    await page.getByTestId('previous').tap();
    await expect(period(page)).toHaveText('2026년 10월');
    await page.getByTestId('go-today').tap();
    await expect(period(page)).toHaveText('2026년 11월');
    await expect(box(page)).toHaveAttribute('data-selected', '2026-11-10');
    expect(errors).toEqual([]);
  });

  test('over the list of days the arrows step by a week', async ({ page }) => {
    await show(page, 'month', 'b');
    const first = page.getByTestId('week-strip').getByRole('button').first();
    await expect(first).toHaveAttribute('data-day', '2026-11-08');
    await page.getByTestId('next').tap();
    await expect(first).toHaveAttribute('data-day', '2026-11-15');
    await expect(page.getByTestId('next')).toHaveAccessibleName('다음 주');
  });

  for (const { screen, variant } of ALL) {
    test(`${screen} ${variant}: everything a finger has to hit is ${TOUCH} pixels or more`, async ({
      page,
    }) => {
      for (const size of [360, 626, 820, 890] as const) {
        await page.setViewportSize(SIZES[size]);
        await show(page, screen, variant);
        expect(await smallTargets(page), `at ${size}`).toEqual([]);
        expect(await spill(page), `at ${size}`).toBe(0);
      }
      expect(errors).toEqual([]);
    });
  }

  test('a field has text of 16 pixels, so that the page is not zoomed', async ({ page }) => {
    await show(page, 'create', 'a');
    const field = page.getByTestId('new-card-title');
    await expect(field).toBeVisible();
    const size = await field.evaluate((element) => parseFloat(getComputedStyle(element).fontSize));
    expect(size).toBeGreaterThanOrEqual(16);
  });

  test('cards without a date are in a bottom sheet, or in a strip under the grid', async ({
    page,
  }) => {
    await show(page, 'display', 'a', '&open=0');
    await page.getByTestId('open-undated').tap();
    const sheet = page.getByTestId('bottom-sheet');
    await expect(sheet).toBeVisible();
    await expect(sheet.locator('[data-card]')).toHaveCount(3);
    expect((await sheet.boundingBox())?.width).toBe(PHONE.width);

    await show(page, 'display', 'b', '&open=0');
    await expect(page.getByTestId('open-undated')).toHaveCount(0);
    const strip = page.getByTestId('undated-strip');
    await expect(strip.locator('[data-card]')).toHaveCount(0);
    await strip.getByRole('button').first().tap();
    await expect(strip.locator('[data-card]')).toHaveCount(3);
  });

  test('"change the date" is in the menu of a card, and leads to a date picker in a sheet', async ({
    page,
  }) => {
    await show(page, 'move', 'b', '&open=menu');
    await expect(
      page.getByTestId('bottom-sheet').getByRole('button', { name: '날짜 바꾸기' }),
    ).toBeVisible();

    await show(page, 'move', 'b');
    const sheet = page.getByTestId('bottom-sheet');
    await expect(sheet.getByRole('heading', { name: '날짜 바꾸기' })).toBeVisible();
    await expect(sheet.getByRole('grid')).toBeVisible();
  });

  test('where bars fit, the whole cell is the button and opens the cards of the day', async ({
    page,
  }) => {
    await page.setViewportSize(SIZES[626]);
    await show(page, 'more', 'a', '&open=0');
    await expect(page.getByTestId('month-grid')).toBeVisible();
    const cell = page.locator('button[data-day="2026-11-12"]');
    await expect(cell).toHaveAccessibleName('11월 12일 목요일, 카드 7개');
    await expect(page.getByTestId('day-popover')).toHaveCount(0);
    // A bar lies over the cell and lets the tap through.
    const bar = await page.locator('[data-bar="c4"]').boundingBox();
    if (!bar) throw new Error('the bar of the card is not drawn');
    await page.touchscreen.tap(bar.x + bar.width / 2, bar.y + bar.height / 2);
    const popover = page.getByTestId('day-popover');
    await expect(popover).toBeVisible();
    await expect(popover.locator('[data-card]')).toHaveCount(7);
    await popover.getByRole('button', { name: '그날의 목록 닫기' }).tap();
    await expect(popover).toBeHidden();
  });

  test('what was in view stays when the window changes size', async ({ page }) => {
    await show(page, 'month', 'a');
    await page.getByTestId('next').tap();
    await day(page, '2026-12-04').tap();
    await expect(box(page)).toHaveAttribute('data-selected', '2026-12-04');

    // Unfolded and turned: the small grid gives way to the grid of bars.
    await page.setViewportSize(SIZES[890]);
    await expect(page.getByTestId('month-grid')).toBeVisible();
    await expect(period(page)).toHaveText('2026년 12월');
    await expect(box(page)).toHaveAttribute('data-selected', '2026-12-04');
    await expect(day(page, '2026-12-04')).toHaveAttribute('aria-pressed', 'true');

    await page.getByTestId('previous').tap();
    await page.setViewportSize(SIZES[360]);
    await expect(page.getByTestId('mini-month')).toBeVisible();
    await expect(period(page)).toHaveText('2026년 11월');
    await expect(page.getByTestId('day-list')).toHaveAttribute('data-date', '2026-11-10');
    expect(await spill(page)).toBe(0);
    expect(errors).toEqual([]);
  });

  test('bars appear when the box of the calendar is 34rem wide', async ({ page }) => {
    await page.setViewportSize({ width: 543, height: 740 });
    await show(page, 'month', 'a');
    await expect(page.getByTestId('mini-month')).toBeVisible();
    await page.setViewportSize({ width: 544, height: 740 });
    await expect(page.getByTestId('month-grid')).toBeVisible();
    await expect(page.getByTestId('mini-month')).toHaveCount(0);
  });
});

test.describe('on a desktop', () => {
  test.skip(({ hasTouch }) => hasTouch, 'the wide layout, with a mouse');

  test('the month is a grid of weeks with a bar a card', async ({ page }) => {
    await show(page, 'month', 'a');
    await expect(page.getByTestId('month-grid')).toBeVisible();
    expect(await weekdays(page)).toEqual(['일', '월', '화', '수', '목', '금', '토']);
    await expect(page.locator('[data-week]')).toHaveCount(5);
    await expect(day(page, '2026-11-10')).toHaveAttribute('aria-current', 'date');
    await expect(day(page, '2026-11-12')).toHaveAccessibleName('11월 12일 목요일, 카드 7개');

    // A card of thirteen days is three bars, one a week, cut square where it goes on.
    const long = page.locator('[data-bar="c3"]');
    await expect(long).toHaveCount(3);
    const first = await long.nth(0).boundingBox();
    const second = await long.nth(1).boundingBox();
    const cell = await page.locator('[data-day="2026-11-15"]').boundingBox();
    expect(Math.round(second?.width ?? 0)).toBe(Math.round((cell?.width ?? 0) * 7));
    expect(Math.round((first?.x ?? 0) + (first?.width ?? 0))).toBe(DESKTOP.width);
  });

  test('the arrows change the month, and the switch shows one week', async ({ page }) => {
    await show(page, 'month', 'a');
    await page.getByTestId('next').click();
    await expect(period(page)).toHaveText('2026년 12월');
    await expect(page.locator('[data-week]')).toHaveCount(5);
    await expect(page.locator('[data-week]').first()).toHaveAttribute('data-week', '2026-11-29');
    await page.getByTestId('go-today').click();
    await expect(period(page)).toHaveText('2026년 11월');

    await page.getByTestId('mode').getByRole('button', { name: '주 보기' }).click();
    await expect(page.locator('[data-week]')).toHaveCount(1);
    await expect(period(page)).toHaveText('11월 8일~14일');
    await expect(page.getByTestId('previous')).toHaveAccessibleName('이전 주');
    expect(errors).toEqual([]);
  });

  test('the week starts on the day the reader has set', async ({ page }) => {
    await show(page, 'month', 'a', '&week=monday');
    expect(await weekdays(page)).toEqual(['월', '화', '수', '목', '금', '토', '일']);
    // November 2026 begins on a Sunday: from Monday it takes six rows.
    await expect(page.locator('[data-week]')).toHaveCount(6);
    await expect(page.locator('[data-week]').first()).toHaveAttribute('data-week', '2026-10-26');

    await show(page, 'month', 'a', '&week=saturday');
    expect(await weekdays(page)).toEqual(['토', '일', '월', '화', '수', '목', '금']);
    await expect(page.locator('[data-week]').first()).toHaveAttribute('data-week', '2026-10-31');
  });

  test('today, and the day of a card with a time, are those of the reader', async ({ page }) => {
    await show(page, 'regional', 'a');
    await expect(day(page, '2026-11-10')).toHaveAttribute('aria-current', 'date');
    await expect(day(page, '2026-11-10')).toHaveAccessibleName('11월 10일 화요일, 카드 2개');
    await expect(day(page, '2026-11-12')).toHaveAccessibleName('11월 12일 목요일, 카드 7개');
    await expect(day(page, '2026-11-21')).toHaveAccessibleName('11월 21일 토요일, 카드 2개');

    // The same moment and the same cards, read in Los Angeles.
    await show(page, 'regional', 'b');
    expect(await weekdays(page)).toEqual(['월', '화', '수', '목', '금', '토', '일']);
    await expect(day(page, '2026-11-09')).toHaveAttribute('aria-current', 'date');
    await expect(day(page, '2026-11-09')).toHaveAccessibleName('11월 9일 월요일, 카드 2개');
    await expect(day(page, '2026-11-10')).toHaveAccessibleName('11월 10일 화요일, 카드 1개');
    await expect(day(page, '2026-11-11')).toHaveAccessibleName('11월 11일 수요일, 카드 3개');
    await expect(day(page, '2026-11-12')).toHaveAccessibleName('11월 12일 목요일, 카드 6개');
    // A card from one evening to the next morning in Seoul is within one day there.
    await expect(day(page, '2026-11-21')).toHaveAccessibleName('11월 21일 토요일, 카드 1개');
  });

  test('"+N" opens the cards of the day in a popover, or in a column beside the grid', async ({
    page,
  }) => {
    await show(page, 'more', 'a', '&open=0');
    const more = page.locator('[data-more="2026-11-12"]');
    await expect(more).toHaveText('+5');
    await more.click();
    const popover = page.getByTestId('day-popover');
    await expect(popover).toBeVisible();
    await expect(popover.locator('[data-card]')).toHaveCount(7);
    await page.keyboard.press('Escape');
    await expect(popover).toBeHidden();

    await show(page, 'more', 'b', '&open=0');
    const before = (await box(page).boundingBox())?.width ?? 0;
    await page.locator('[data-more="2026-11-12"]').click();
    const panel = page.getByTestId('day-panel');
    await expect(panel).toBeVisible();
    await expect(panel.locator('[data-card]')).toHaveCount(7);
    // Beside the grid, which gives it 20rem; nothing of it on the middle line.
    expect((await panel.boundingBox())?.x).toBe(DESKTOP.width - 320);
    expect((await box(page).boundingBox())?.width).toBe(before - 320);
    await expect(page.getByTestId('month-grid')).toBeVisible();
    expect(await spill(page)).toBe(0);
  });

  test('in a pane narrower than 54rem the column lies over the grid', async ({ page }) => {
    await page.setViewportSize(SIZES[890]);
    await show(page, 'more', 'b');
    const panel = page.getByTestId('day-panel');
    await expect(panel).toBeVisible();
    // 890 less the fixed sidebar: the grid keeps the whole pane.
    expect((await box(page).boundingBox())?.width).toBe(890 - 256);
    expect((await panel.boundingBox())?.x).toBe(890 - 320);
  });

  test('a card is made by a field in the cell, or from a "+" that shows when pointed at', async ({
    page,
  }) => {
    await show(page, 'create', 'a');
    const field = page.getByTestId('new-card-title');
    await expect(field).toBeVisible();
    const cell = await page.locator('[data-day="2026-11-25"]').boundingBox();
    const placed = await field.boundingBox();
    expect(placed?.x).toBeGreaterThanOrEqual(cell?.x ?? Infinity);
    await expect(page.locator('[data-new-on]')).toHaveCount(0);

    await show(page, 'create', 'b');
    await expect(page.locator('[data-new-on="2026-11-25"]')).toHaveCSS('opacity', '1');
    const other = page.locator('[data-new-on="2026-11-18"]');
    await expect(other).toHaveCSS('opacity', '0');
    await page.locator('[data-day="2026-11-18"]').hover();
    await expect(other).toHaveCSS('opacity', '1');
    await expect(other).toHaveAccessibleName('11월 18일에 새 카드');
  });

  test('cards without a date are in a column that opens, or in a strip under the grid', async ({
    page,
  }) => {
    await show(page, 'display', 'a', '&open=0');
    await expect(page.getByTestId('undated-panel')).toHaveCount(0);
    await page.getByTestId('open-undated').click();
    await expect(page.getByTestId('undated-panel').locator('[data-card]')).toHaveCount(3);

    await show(page, 'display', 'b');
    await expect(page.getByTestId('undated-strip').locator('[data-card]')).toHaveCount(3);
  });

  test('the property the cards are placed by is chosen from the header', async ({ page }) => {
    await show(page, 'display', 'a', '&open=by');
    const list = page.getByTestId('display-by');
    await expect(list.getByRole('button')).toHaveCount(4);
    await expect(list.getByRole('button', { pressed: true })).toContainText('일정');
    await expect(page.getByTestId('display-by-button')).toHaveText('표시 기준: 일정');
  });

  test('the menu of a bar has "change the date", which opens a date picker beside it', async ({
    page,
  }) => {
    await show(page, 'move', 'b', '&open=menu');
    await expect(
      page.getByTestId('dropdown').getByRole('menuitem', { name: '날짜 바꾸기' }),
    ).toBeVisible();
    await show(page, 'move', 'b');
    await expect(page.getByTestId('popover').getByRole('grid')).toBeVisible();
  });

  test('a mouse in a narrow window gets the narrow layout with small targets', async ({ page }) => {
    await page.setViewportSize(PHONE);
    await show(page, 'month', 'a');
    await expect(page.getByTestId('mini-month')).toBeVisible();
    expect((await page.getByTestId('next').boundingBox())?.height).toBe(32);
    expect(await spill(page)).toBe(0);
  });
});
