/**
 * The mobile mock-ups in a browser: every screen and proposal comes up, the
 * narrow ones do not spill sideways, and each proposal is in fact the thing
 * it claims to be (a sheet, a popover, a page, a dialog).
 *
 * The `touch` project looks at the phone (360×740, a finger), the `desktop`
 * project at the desktop (1280×800, a mouse).
 */
import { expect, test, type Page } from '@playwright/test';

import { SCREENS, addressOf, type ScreenId, type Variant } from './params.ts';
import { DESKTOP, NOW, PAGE, PHONE, settled } from './support.ts';

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

async function show(page: Page, screen: ScreenId, variant: Variant, extra = ''): Promise<void> {
  await page.goto(PAGE + addressOf(screen, variant, extra));
  await expect(page.getByTestId('view-header')).toBeVisible();
  await settled(page);
}

/** How far the document can be scrolled sideways. Nothing but a marked region may scroll that way. */
async function spill(page: Page): Promise<number> {
  return page.evaluate(() => {
    const root = document.documentElement;
    return Math.max(root.scrollWidth, document.body.scrollWidth) - root.clientWidth;
  });
}

/**
 * The buttons in view that a finger would miss: narrower or lower than the
 * smallest target. A switch inside its label is as large as the label, which
 * passes a tap on to it.
 */
async function smallTargets(page: Page, within: string): Promise<string[]> {
  return page.locator(`${within} button`).evaluateAll(
    (buttons, least) =>
      buttons.flatMap((button) => {
        const box = (button.closest('label') ?? button).getBoundingClientRect();
        if (box.width === 0 || box.height === 0) return [];
        if (box.width >= least && box.height >= least) return [];
        const name = button.getAttribute('aria-label') ?? button.textContent.trim();
        return [`${name} ${Math.round(box.width)}x${Math.round(box.height)}`];
      }),
    TOUCH,
  );
}

test.describe('every screen and proposal', () => {
  for (const { screen, variant } of ALL) {
    test(`${screen} ${variant} comes up without an error and without spilling sideways`, async ({
      page,
    }) => {
      await show(page, screen, variant);
      expect(errors).toEqual([]);
      expect(await spill(page)).toBe(0);
      expect(await page.locator('html').getAttribute('lang')).toBe('ko');
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

test.describe('on a phone', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'the narrow layout, with a finger');

  test('the sidebar is a drawer that the back button closes', async ({ page }) => {
    await show(page, 'sidebar', 'a');
    const drawer = page.locator('[data-sidebar="sidebar"][data-mobile="true"]');
    await expect(drawer).toBeVisible();
    const box = await drawer.boundingBox();
    expect(box?.x).toBe(0);
    expect(box?.width).toBeLessThan(PHONE.width);

    await page.goBack();
    await expect(drawer).toBeHidden();
    await expect(page).toHaveURL(/screen=sidebar/);

    await page.getByTestId('open-sidebar').tap();
    await expect(drawer).toBeVisible();
  });

  test('the board switcher is a bottom sheet opened from the name of the board', async ({
    page,
  }) => {
    await show(page, 'sidebar', 'b', '&open=0');
    await expect(page.getByTestId('open-sidebar')).toHaveCount(0);
    await page.getByTestId('open-switcher').tap();
    const sheet = page.getByTestId('bottom-sheet');
    await expect(sheet).toBeVisible();
    await expect(sheet.getByRole('button', { name: '버그 트래커' })).toBeVisible();
  });

  test('the kanban that snaps rests on one column and names it in the tabs', async ({ page }) => {
    await show(page, 'kanban', 'a', '&column=0');
    const tabs = page.getByTestId('column-tabs').getByRole('tab');
    await expect(tabs.nth(0)).toHaveAttribute('aria-selected', 'true');

    await tabs.nth(2).tap();
    await expect(tabs.nth(2)).toHaveAttribute('aria-selected', 'true');
    // The column is in the middle of the screen, a sliver of each neighbour beside it.
    const column = page.locator('[data-column="review"]');
    await expect
      .poll(async () => {
        const box = await column.boundingBox();
        return box ? Math.round(box.x + box.width / 2) : -1;
      })
      .toBe(PHONE.width / 2);
    const before = await page.locator('[data-column="doing"]').boundingBox();
    expect((before?.x ?? 0) + (before?.width ?? 0)).toBeGreaterThan(0);
  });

  test('the kanban that scrolls freely has no tabs and shows part of the next column', async ({
    page,
  }) => {
    await show(page, 'kanban', 'b');
    await expect(page.getByTestId('column-tabs')).toHaveCount(0);
    const second = await page.locator('[data-column="doing"]').boundingBox();
    expect(second?.x).toBeLessThan(PHONE.width);
    expect((second?.x ?? 0) + (second?.width ?? 0)).toBeGreaterThan(PHONE.width);
  });

  test('the table keeps its title column in place while the others scroll', async ({ page }) => {
    await show(page, 'table', 'a');
    const table = page.getByTestId('table');
    const title = table.getByRole('button', { name: '다크 모드' });
    const status = table.getByRole('columnheader', { name: '상태' });
    const before = await status.boundingBox();

    await table.evaluate((element) => {
      element.scrollLeft = 200;
    });
    await expect.poll(async () => (await status.boundingBox())?.x).toBe((before?.x ?? 0) - 200);
    expect((await title.boundingBox())?.x).toBe(0);
    expect(await spill(page)).toBe(0);
  });

  test('the table as a list has no columns and nothing to scroll sideways', async ({ page }) => {
    await show(page, 'table', 'b');
    await expect(page.getByTestId('table')).toHaveCount(0);
    const list = page.getByTestId('table-list');
    await expect(list.getByRole('button').first()).toBeVisible();
    expect(await list.evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
  });

  test('a card is a page that fills the screen, and the back button closes it', async ({
    page,
  }) => {
    await show(page, 'card', 'a');
    const card = page.getByTestId('card-page');
    await expect(card).toBeVisible();
    expect(await card.boundingBox()).toEqual({ x: 0, y: 0, ...PHONE });

    await page.goBack();
    await expect(card).toBeHidden();
    await expect(page).toHaveURL(/screen=card/);

    // Opened by hand, closed with its own arrow: the next "back" is not swallowed.
    await page.locator('[data-card="c1"]').tap();
    await expect(card).toBeVisible();
    await card.getByRole('button', { name: '뒤로' }).tap();
    await expect(card).toBeHidden();
  });

  test('a card as a sheet leaves a strip of the board above it', async ({ page }) => {
    await show(page, 'card', 'b');
    const card = page.getByTestId('card-sheet');
    await expect(card).toBeVisible();
    const box = await card.boundingBox();
    expect(box?.y).toBe(40);
    expect((box?.y ?? 0) + (box?.height ?? 0)).toBe(PHONE.height);
  });

  for (const screen of ['menu-view', 'menu-card', 'menu-select', 'menu-date'] as const) {
    test(`${screen}: a bottom sheet in one proposal, a popover in the other`, async ({ page }) => {
      await show(page, screen, 'a');
      const sheet = page.getByTestId('bottom-sheet');
      await expect(sheet).toBeVisible();
      const box = await sheet.boundingBox();
      expect(box?.x).toBe(0);
      expect(box?.width).toBe(PHONE.width);
      expect((box?.y ?? 0) + (box?.height ?? 0)).toBe(PHONE.height);
      expect(await smallTargets(page, '[data-testid="bottom-sheet"]')).toEqual([]);

      await show(page, screen, 'b');
      await expect(page.getByTestId('bottom-sheet')).toHaveCount(0);
      const popup = page.getByTestId(screen === 'menu-card' ? 'dropdown' : 'popover');
      await expect(popup).toBeVisible();
      const popupBox = await popup.boundingBox();
      expect(popupBox?.x).toBeGreaterThanOrEqual(0);
      expect((popupBox?.x ?? 0) + (popupBox?.width ?? 0)).toBeLessThanOrEqual(PHONE.width);
    });
  }

  test('a sheet closes with the back button and leaves the card under it open', async ({
    page,
  }) => {
    await show(page, 'menu-select', 'a');
    await expect(page.getByTestId('bottom-sheet')).toBeVisible();
    await page.goBack();
    await expect(page.getByTestId('bottom-sheet')).toBeHidden();
    await expect(page.getByTestId('card-page')).toBeVisible();
    await page.goBack();
    await expect(page.getByTestId('card-page')).toBeHidden();
  });

  test('the folded header is one row in one proposal and two in the other', async ({ page }) => {
    /** Buttons that reach past the right edge of the screen: clipped, not scrolled to. */
    const cutOff = (within: string): Promise<string[]> =>
      page
        .locator(`[data-testid="${within}"] button`)
        .evaluateAll((buttons) =>
          buttons
            .filter((button) => button.getBoundingClientRect().right > window.innerWidth)
            .map((button) => button.getAttribute('aria-label') ?? button.textContent.trim()),
        );

    await show(page, 'header', 'a');
    const one = await page.getByTestId('view-header').boundingBox();
    await expect(page.getByTestId('new-card-fab')).toBeVisible();
    expect(await cutOff('view-header')).toEqual([]);

    await show(page, 'header', 'b');
    const two = await page.getByTestId('view-header').boundingBox();
    await expect(page.getByTestId('new-card-fab')).toHaveCount(0);
    await expect(
      page.getByTestId('view-header').getByRole('button', { name: '새로 만들기' }),
    ).toBeVisible();
    expect(two?.height).toBeGreaterThan((one?.height ?? 0) + TOUCH - 1);
    // The row of tabs scrolls sideways on purpose; the row of tools must fit.
    expect(await cutOff('view-tools')).toEqual([]);
  });

  test('what a finger has to hit is at least the smallest target', async ({ page }) => {
    await show(page, 'kanban', 'a');
    for (const region of ['app-bar', 'view-header', 'column-tabs', 'kanban']) {
      expect(await smallTargets(page, `[data-testid="${region}"]`), region).toEqual([]);
    }
    await show(page, 'header', 'b');
    expect(await smallTargets(page, '[data-testid="view-header"]')).toEqual([]);
    await show(page, 'card', 'a');
    expect(await smallTargets(page, '[data-testid="card-page"]')).toEqual([]);
  });

  test('a field is 16 pixels of text or more, so that iOS does not zoom', async ({ page }) => {
    await show(page, 'menu-select', 'a');
    const sizes = await page
      .locator('input:visible')
      .evaluateAll((inputs) => inputs.map((input) => parseFloat(getComputedStyle(input).fontSize)));
    expect(sizes.length).toBeGreaterThan(0);
    expect(Math.min(...sizes)).toBeGreaterThanOrEqual(16);
  });
});

test.describe('on a desktop', () => {
  test.skip(({ hasTouch }) => hasTouch, 'the wide layout, with a mouse');

  test('the sidebar is fixed, and boards are also reached by a command palette', async ({
    page,
  }) => {
    await show(page, 'sidebar', 'b');
    await expect(page.locator('[data-slot="sidebar-container"]')).toBeVisible();
    await expect(page.getByRole('dialog').getByPlaceholder('보드 찾기')).toBeVisible();
    await expect(page.getByTestId('bottom-sheet')).toHaveCount(0);
  });

  test('the whole header is one row, and the kanban has no column tabs', async ({ page }) => {
    await show(page, 'kanban', 'a');
    const header = page.getByTestId('view-header');
    await expect(header.getByRole('button', { name: '새로 만들기' })).toBeVisible();
    await expect(header.getByRole('button', { name: '정렬' })).toBeVisible();
    await expect(page.getByTestId('column-tabs')).toHaveCount(0);
    await expect(page.getByTestId('new-card-fab')).toHaveCount(0);
    expect(await header.evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
  });

  test('a card is a dialog in one proposal and a panel at the right in the other', async ({
    page,
  }) => {
    await show(page, 'card', 'a');
    const dialog = await page.getByTestId('card-dialog').boundingBox();
    expect(dialog?.x).toBeGreaterThan(100);
    expect((dialog?.x ?? 0) + (dialog?.width ?? 0)).toBeLessThan(DESKTOP.width - 100);

    await show(page, 'card', 'b');
    const panel = await page.getByTestId('card-sheet').boundingBox();
    expect((panel?.x ?? 0) + (panel?.width ?? 0)).toBe(DESKTOP.width);
    expect(panel?.height).toBe(DESKTOP.height);
  });

  for (const screen of ['menu-view', 'menu-card', 'menu-select', 'menu-date'] as const) {
    test(`${screen} is a popover next to what opened it, in both proposals`, async ({ page }) => {
      for (const variant of ['a', 'b'] as const) {
        await show(page, screen, variant);
        await expect(page.getByTestId('bottom-sheet')).toHaveCount(0);
        await expect(
          page.getByTestId(screen === 'menu-card' ? 'dropdown' : 'popover'),
        ).toBeVisible();
      }
    });
  }

  test('a narrow window with a mouse gets the narrow layout at the sizes of a mouse', async ({
    page,
  }) => {
    await page.setViewportSize(PHONE);
    await show(page, 'kanban', 'a');
    await expect(page.getByTestId('column-tabs')).toBeVisible();
    const button = await page.getByTestId('open-sidebar').boundingBox();
    expect(button?.width).toBe(32);
    expect(await spill(page)).toBe(0);
  });
});
