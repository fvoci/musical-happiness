/**
 * The mobile mock-ups in a browser: every screen and proposal comes up, the
 * narrow ones do not spill sideways, and each proposal is in fact the thing
 * it claims to be (a sheet, a popover, a page, a dialog).
 *
 * The `touch` project looks at the phone (360×740, a finger) and at the
 * sizes in between; the `desktop` project at the desktop (1280×800, a mouse).
 *
 * Further down: what changes at which width and that classes and script
 * agree on it, what happens to what is open when the window changes size,
 * and the edges a device keeps for itself.
 */
import { expect, test, type Page } from '@playwright/test';

import { SCREENS, SIZES, addressOf, type ScreenId, type Variant } from './params.ts';
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
    const drawer = page.getByTestId('sidebar-drawer');
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
    await expect(page.getByTestId('sidebar')).toBeVisible();
    await expect(page.getByRole('dialog').getByPlaceholder('보드 찾기')).toBeVisible();
    await expect(page.getByTestId('bottom-sheet')).toHaveCount(0);
  });

  test('the whole header is one row, and the kanban has no column tabs', async ({ page }) => {
    await show(page, 'kanban', 'a');
    const header = page.getByTestId('view-header');
    await expect(header.getByRole('button', { name: '새로 만들기' })).toBeVisible();
    await expect(header.getByRole('button', { name: '정렬' })).toBeVisible();
    await expect(page.getByTestId('column-tabs')).toBeHidden();
    await expect(page.getByTestId('new-card-fab')).toHaveCount(0);
    expect(await header.evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
  });

  test('a card is a dialog in one proposal and a pane beside the board in the other', async ({
    page,
  }) => {
    await show(page, 'card', 'a');
    const dialog = await page.getByTestId('card-dialog').boundingBox();
    expect(dialog?.x).toBeGreaterThan(100);
    expect((dialog?.x ?? 0) + (dialog?.width ?? 0)).toBeLessThan(DESKTOP.width - 100);

    await show(page, 'card', 'b');
    const beside = await page.getByTestId('card-pane').boundingBox();
    // Half of the page: its left edge is on the middle line.
    expect(beside?.x).toBe(DESKTOP.width / 2);
    expect((beside?.x ?? 0) + (beside?.width ?? 0)).toBe(DESKTOP.width);
    expect(beside?.height).toBe(DESKTOP.height);
    // The board is still there, in what is left, and laid out for that width.
    const board = await page.getByTestId('pane').boundingBox();
    expect(board?.width).toBe(DESKTOP.width / 2 - 256);
    await expect(page.getByTestId('column-tabs')).toBeVisible();
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

/** The stage the page says it is in: what script made of the width. */
async function expectStage(page: Page, stage: 'compact' | 'medium' | 'expanded'): Promise<void> {
  await expect(page.getByTestId('app')).toHaveAttribute('data-stage', stage);
}

async function widthOf(page: Page, name: string): Promise<number> {
  return (await page.getByTestId(name).boundingBox())?.width ?? -1;
}

/** Whether a column of the kanban is in view from edge to edge. */
async function wholeInView(page: Page, column: string): Promise<boolean> {
  return page.locator(`[data-column="${column}"]`).evaluate((element) => {
    const box = element.getBoundingClientRect();
    const within = element.closest('[data-testid="pane"]')?.getBoundingClientRect();
    return within !== undefined && box.left >= within.left && box.right <= within.right;
  });
}

test.describe('the sizes in between', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'with a finger');

  for (const width of [466, 626, 820, 890] as const) {
    test(`every screen and proposal comes up at ${width} without an error or spilling sideways`, async ({
      page,
    }) => {
      test.setTimeout(120_000);
      await page.setViewportSize(SIZES[width]);
      for (const { screen, variant } of ALL) {
        await show(page, screen, variant);
        expect(errors, `${screen} ${variant}`).toEqual([]);
        expect(await spill(page), `${screen} ${variant}`).toBe(0);
      }
    });
  }

  test('466 is compact: one column, a drawer, a card as a page, a menu as a sheet', async ({
    page,
  }) => {
    await page.setViewportSize(SIZES[466]);
    await show(page, 'kanban', 'a');
    await expectStage(page, 'compact');
    await expect(page.getByTestId('column-tabs')).toBeVisible();
    await expect(page.getByTestId('open-sidebar')).toBeVisible();
    // The one column does not grow without end: 24rem, in the middle.
    expect((await page.locator('[data-column="doing"]').boundingBox())?.width).toBe(384);

    await show(page, 'card', 'a');
    expect(await page.getByTestId('card-page').boundingBox()).toEqual({
      x: 0,
      y: 0,
      ...SIZES[466],
    });
    await show(page, 'menu-card', 'a');
    expect(await widthOf(page, 'bottom-sheet')).toBe(466);
  });

  test('626 is medium: a strip, two columns, a card as a panel, a menu as a popover', async ({
    page,
  }) => {
    await page.setViewportSize(SIZES[626]);
    await show(page, 'kanban', 'a');
    await expectStage(page, 'medium');
    expect(await widthOf(page, 'sidebar-strip')).toBe(56);
    expect(await widthOf(page, 'pane')).toBe(570);
    await expect(page.getByTestId('column-tabs')).toBeHidden();
    await expect(page.getByTestId('open-sidebar')).toHaveCount(0);
    expect(await wholeInView(page, 'todo')).toBe(true);
    expect(await wholeInView(page, 'doing')).toBe(true);
    // The header is still folded: the pane is far from 62rem.
    await expect(page.getByTestId('new-card-fab')).toBeVisible();
    expect(await smallTargets(page, '[data-testid="sidebar-strip"]')).toEqual([]);

    // The strip opens into the whole list, over the page.
    await page.getByTestId('expand-sidebar').tap();
    const drawer = page.getByTestId('sidebar-drawer');
    await expect(drawer).toBeVisible();
    expect(Math.round((await drawer.boundingBox())?.width ?? 0)).toBe(288);
    await page.goBack();
    await expect(drawer).toBeHidden();

    // The other proposal: no strip, the button of a phone, the same drawer.
    await show(page, 'sidebar', 'b', '&open=0');
    await expect(page.getByTestId('sidebar-strip')).toHaveCount(0);
    expect(await widthOf(page, 'pane')).toBe(626);
    await page.getByTestId('open-sidebar').tap();
    await expect(drawer).toBeVisible();

    for (const variant of ['a', 'b'] as const) {
      await show(page, 'card', variant);
      const panel = await page.getByTestId('card-panel').boundingBox();
      expect(panel).toEqual({ x: 626 - 480, y: 0, width: 480, height: 890 });
    }

    await show(page, 'menu-select', 'a');
    await expect(page.getByTestId('popover')).toBeVisible();
    await expect(page.getByTestId('bottom-sheet')).toHaveCount(0);
  });

  test('820 is medium as well, with room for more of the same', async ({ page }) => {
    await page.setViewportSize(SIZES[820]);
    await show(page, 'kanban', 'a');
    await expectStage(page, 'medium');
    expect(await widthOf(page, 'pane')).toBe(764);
    expect(await wholeInView(page, 'doing')).toBe(true);
    expect(await wholeInView(page, 'review')).toBe(false);
    // A finger on a wide screen: what it hits is large, and a field does not make iOS zoom.
    await show(page, 'card', 'a');
    expect(await smallTargets(page, '[data-testid="card-panel"]')).toEqual([]);
    const size = await page
      .getByTestId('comment-field')
      .evaluate((field) => parseFloat(getComputedStyle(field).fontSize));
    expect(size).toBeGreaterThanOrEqual(16);
  });

  test('890 is expanded, and what the pane shows depends on what is beside it', async ({
    page,
  }) => {
    await page.setViewportSize(SIZES[890]);
    await show(page, 'kanban', 'a');
    await expectStage(page, 'expanded');
    expect(await widthOf(page, 'sidebar')).toBe(256);
    expect(await widthOf(page, 'pane')).toBe(634);
    expect(await wholeInView(page, 'doing')).toBe(true);
    expect(await wholeInView(page, 'review')).toBe(false);
    await expect(page.getByTestId('new-card-fab')).toBeVisible();

    // The sidebar folded to a strip: the same window, a third column.
    await page.getByTestId('toggle-sidebar').tap();
    await expect(page.getByTestId('sidebar-strip')).toBeVisible();
    await expect.poll(() => widthOf(page, 'pane')).toBe(834);
    expect(await wholeInView(page, 'review')).toBe(true);
    await page.getByTestId('expand-sidebar').tap();
    await expect(page.getByTestId('sidebar')).toBeVisible();

    await show(page, 'card', 'a');
    await expect(page.getByTestId('card-dialog')).toBeVisible();

    // Beside the board: half the page each, and the sidebar gives way to a strip.
    await show(page, 'card', 'b');
    const beside = await page.getByTestId('card-pane').boundingBox();
    expect(beside?.x).toBe(445);
    expect(beside?.width).toBe(445);
    await expect(page.getByTestId('sidebar-strip')).toBeVisible();
    expect(await widthOf(page, 'pane')).toBe(389);
    await expect(page.getByTestId('column-tabs')).toBeVisible();
    // Closed, the sidebar comes back and the board has its columns again.
    await page.getByTestId('card-pane').getByRole('button', { name: '닫기' }).tap();
    await expect(page.getByTestId('sidebar')).toBeVisible();
    await expect(page.getByTestId('column-tabs')).toBeHidden();
  });
});

test.describe('where the layout changes', () => {
  test.skip(({ hasTouch }) => hasTouch, 'widths do not depend on the pointer: one project');

  test('the page is medium from 600 and expanded from 840', async ({ page }) => {
    const stages = [
      [599, 'compact'],
      [600, 'medium'],
      [839, 'medium'],
      [840, 'expanded'],
    ] as const;
    for (const [width, stage] of stages) {
      await page.setViewportSize({ width, height: 800 });
      await show(page, 'kanban', 'a');
      await expectStage(page, stage);
    }
  });

  test('classes and script change at the same width of the pane', async ({ page }) => {
    // 34rem. A class hides the column tabs; script turns the list into a table.
    await page.setViewportSize({ width: 543, height: 800 });
    await show(page, 'table', 'b');
    await expect(page.getByTestId('table-list')).toBeVisible();
    await show(page, 'kanban', 'a');
    await expect(page.getByTestId('column-tabs')).toBeVisible();
    await page.setViewportSize({ width: 544, height: 800 });
    await expect(page.getByTestId('column-tabs')).toBeHidden();
    await show(page, 'table', 'b');
    await expect(page.getByTestId('table')).toBeVisible();

    // 62rem, which beside a sidebar of 256 is a window of 1248. A class shows
    // the description of the board; script unfolds the header.
    const description = page.getByText('앱 4.0에 들어갈 작업을 한곳에서 봅니다.');
    await page.setViewportSize({ width: 1247, height: 800 });
    await show(page, 'kanban', 'a');
    await expect(description).toBeHidden();
    await expect(page.getByTestId('new-card-fab')).toBeVisible();
    await page.setViewportSize({ width: 1248, height: 800 });
    await expect(description).toBeVisible();
    await expect(page.getByTestId('new-card-fab')).toHaveCount(0);
  });

  test('the pane follows the sidebar, not the window', async ({ page }) => {
    await page.setViewportSize({ width: 1100, height: 800 });
    await show(page, 'kanban', 'a');
    const header = page.getByTestId('view-header');
    expect(await widthOf(page, 'pane')).toBe(844);
    await expect(header.getByRole('button', { name: '정렬' })).toHaveCount(0);

    await page.getByTestId('toggle-sidebar').click();
    await expect.poll(() => widthOf(page, 'pane')).toBe(1044);
    await expect(header.getByRole('button', { name: '정렬' })).toBeVisible();
    await expect(page.getByTestId('new-card-fab')).toHaveCount(0);
  });

  test('a larger text size moves every boundary with it', async ({ page }) => {
    await page.setViewportSize({ width: 700, height: 800 });
    await show(page, 'kanban', 'a');
    await expectStage(page, 'medium');
    // 25% larger: 37.5rem is now 750 pixels, 34rem is 680.
    await page.evaluate(() => {
      document.documentElement.style.fontSize = '20px';
    });
    await page.setViewportSize({ width: 701, height: 800 });
    await expectStage(page, 'compact');
    await expect(page.getByTestId('column-tabs')).toBeHidden();
    await page.setViewportSize({ width: 679, height: 800 });
    await expect(page.getByTestId('column-tabs')).toBeVisible();
  });

  test('the "…" of a card waits to be pointed at, where the device can point', async ({ page }) => {
    await show(page, 'kanban', 'a');
    const card = page.locator('[data-card="c1"]');
    const menu = card.getByRole('button', { name: '카드 메뉴' });
    await expect(menu).toHaveCSS('opacity', '0');
    await card.hover();
    await expect(menu).toHaveCSS('opacity', '1');
  });
});

test.describe('when the window changes size', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'a device that folds and turns: with a finger');

  const DRAFT = '반쯤 쓴 댓글';

  test('a card over the board stays as it is: the draft, the scroll, the caret', async ({
    page,
  }) => {
    await show(page, 'card', 'a');
    const field = page.getByTestId('comment-field');
    const scroller = page.getByTestId('card-scroll');
    await field.fill(DRAFT);
    await scroller.evaluate((element) => {
      element.scrollTop = 120;
      // The very element, to tell a card that was kept from one that was built again.
      Object.assign(window, { spikeScroller: element });
    });

    const forms = [
      [626, 'card-panel'],
      [890, 'card-dialog'],
      [820, 'card-panel'],
      [360, 'card-page'],
    ] as const;
    for (const [width, name] of forms) {
      await page.setViewportSize(SIZES[width]);
      await expect(page.getByTestId(name)).toBeVisible();
      await expect(field).toHaveValue(DRAFT);
      await expect(field).toBeFocused();
      expect(
        await scroller.evaluate((element) => ({
          at: element.scrollTop,
          same: element === (window as unknown as { spikeScroller: Element }).spikeScroller,
        })),
        name,
      ).toEqual({ at: 120, same: true });
    }

    // Still one step of the history: one "back" closes it.
    await page.goBack();
    await expect(page.getByTestId('card-page')).toBeHidden();
    await expect(page).toHaveURL(/screen=card/);
  });

  test('a card beside the board is built again: the draft and the scroll are carried, the focus is not', async ({
    page,
  }) => {
    await show(page, 'card', 'b');
    const field = page.getByTestId('comment-field');
    const scroller = page.getByTestId('card-scroll');
    await expect(page.getByTestId('card-sheet')).toBeVisible();
    await field.fill(DRAFT);
    await scroller.evaluate((element) => {
      element.scrollTop = 120;
    });
    await expect(field).toBeFocused();

    const forms = [
      [626, 'card-panel'],
      [890, 'card-pane'],
      [360, 'card-sheet'],
    ] as const;
    for (const [width, name] of forms) {
      await page.setViewportSize(SIZES[width]);
      await expect(page.getByTestId(name)).toBeVisible();
      await expect(field).toHaveValue(DRAFT);
      await expect.poll(() => scroller.evaluate((element) => element.scrollTop), name).toBe(120);
      await expect(field).not.toBeFocused();
    }
  });

  test('a drawer that is out stays out until the list is on the page, and does not come back', async ({
    page,
  }) => {
    await show(page, 'sidebar', 'a');
    const drawer = page.getByTestId('sidebar-drawer');
    await expect(drawer).toBeVisible();

    await page.setViewportSize(SIZES[626]);
    await expectStage(page, 'medium');
    await expect(drawer).toBeVisible();

    await page.setViewportSize(SIZES[890]);
    await expectStage(page, 'expanded');
    await expect(drawer).toBeHidden();
    await expect(page.getByTestId('sidebar')).toBeVisible();

    await page.setViewportSize(SIZES[626]);
    await expect(page.getByTestId('sidebar-strip')).toBeVisible();
    await expect(drawer).toBeHidden();
    // The step of the history the drawer had taken was given back when it closed.
    await expect(page).toHaveURL(/screen=sidebar/);
  });

  test('an open sheet becomes a popover and a sheet again, open all along', async ({ page }) => {
    await show(page, 'menu-select', 'a');
    await expect(page.getByTestId('bottom-sheet')).toBeVisible();

    await page.setViewportSize(SIZES[626]);
    await expect(page.getByTestId('popover')).toBeVisible();
    await expect(page.getByTestId('bottom-sheet')).toHaveCount(0);
    await expect(page.getByTestId('card-panel')).toBeVisible();

    await page.setViewportSize(SIZES[360]);
    await expect(page.getByTestId('bottom-sheet')).toBeVisible();
    await expect(page.getByTestId('popover')).toHaveCount(0);
    expect(errors).toEqual([]);
  });

  test('the board keeps its place: a table where it was scrolled to, a kanban on its column', async ({
    page,
  }) => {
    await show(page, 'table', 'a');
    const table = page.getByTestId('table');
    const place = (): Promise<{ left: number; top: number }> =>
      table.evaluate((element) => ({ left: element.scrollLeft, top: element.scrollTop }));
    await table.evaluate((element) => {
      element.scrollTo(100, 60);
    });

    // Wider, and still not wide enough for the whole table: sideways it stays.
    // Taller than the table is long: there is nothing left to scroll down.
    await page.setViewportSize(SIZES[626]);
    await expectStage(page, 'medium');
    expect(await place()).toEqual({ left: 100, top: 0 });

    // What the spike does not keep. On the way to 890 there is one pass of
    // layout in which the window is already wide but the page is still put
    // together for the medium stage: the strip of 56 pixels, not the fixed
    // sidebar. The pane is 834 wide in that pass, the table all but fits, and
    // the browser brings the scroll back to the little that is left to
    // scroll. There it stays when the sidebar takes its 256 pixels.
    await page.setViewportSize(SIZES[890]);
    await expectStage(page, 'expanded');
    expect(await widthOf(page, 'pane')).toBe(634);
    const tableWidth = await table.evaluate((element) => element.scrollWidth);
    expect(tableWidth).toBeGreaterThan(634 + 100);
    expect((await place()).left).toBe(tableWidth - (890 - 56));

    await page.setViewportSize(SIZES[360]);
    await show(page, 'kanban', 'a', '&column=2');
    const tabs = page.getByTestId('column-tabs').getByRole('tab');
    await expect(tabs.nth(2)).toHaveAttribute('aria-selected', 'true');
    await page.setViewportSize(SIZES[626]);
    await expect(page.getByTestId('column-tabs')).toBeHidden();
    // Back to one column: the one that was in view, in the middle again.
    await page.setViewportSize(SIZES[360]);
    await expect(tabs.nth(2)).toHaveAttribute('aria-selected', 'true');
    await expect
      .poll(async () => {
        const box = await page.locator('[data-column="review"]').boundingBox();
        return box ? Math.round(box.x + box.width / 2) : -1;
      })
      .toBe(180);
  });

  test('turned on its side, the same page is laid out for the new width and height', async ({
    page,
  }) => {
    await page.setViewportSize(SIZES[626]);
    await show(page, 'card', 'a');
    await expect(page.getByTestId('card-panel')).toBeVisible();
    await page.setViewportSize(SIZES[890]);
    // Heights come from `dvh`, read anew: the dialog fits the lower screen.
    const dialog = page.getByTestId('card-dialog');
    await expect
      .poll(async () => ({
        dialog: (await dialog.boundingBox())?.height,
        window: await page.evaluate(() => window.innerHeight),
      }))
      .toEqual({ dialog: 626 - 64, window: 626 });
    expect(await spill(page)).toBe(0);
  });
});

test.describe('the edges a device keeps for itself', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'with a finger');

  /** Buttons and fields of these parts that reach into the strip at the right. */
  const underTheStrip = (page: Page, edge: number): Promise<string[]> =>
    page
      .locator(
        [
          'app-bar',
          'view-header',
          'new-card-fab',
          'bottom-sheet',
          'sidebar-drawer',
          'card-page',
          'popover',
          'dropdown',
        ]
          .map((name) => `[data-testid="${name}"]`)
          .flatMap((part) => [`${part} button`, `${part} input`, `button${part}`])
          .join(', '),
      )
      .evaluateAll(
        (controls, limit) =>
          controls
            .filter((control) => {
              const box = control.getBoundingClientRect();
              return box.width > 0 && box.right > limit;
            })
            .map((control) => control.getAttribute('aria-label') ?? control.textContent.trim()),
        edge,
      );

  test('84 pixels at the right only: the page is 382 wide and starts at the left edge', async ({
    page,
  }) => {
    await page.setViewportSize(SIZES[466]);
    const screens = [
      ['kanban', 'a'],
      ['sidebar', 'a'],
      ['card', 'a'],
      ['menu-card', 'a'],
      ['menu-select', 'a'],
      ['menu-select', 'b'],
      ['menu-card', 'b'],
    ] as const;
    for (const [screen, variant] of screens) {
      await show(page, screen, variant, '&safe=right');
      const name = `${screen} ${variant}`;
      expect(await page.getByTestId('app').boundingBox(), name).toMatchObject({ x: 0, width: 382 });
      expect(await underTheStrip(page, 382), name).toEqual([]);
      await expectStage(page, 'compact');
    }

    // Nothing was moved in from the left by the room kept at the right.
    await show(page, 'kanban', 'a', '&safe=right');
    expect((await page.getByTestId('open-sidebar').boundingBox())?.x).toBeLessThan(8);
    await show(page, 'menu-card', 'a', '&safe=right');
    const sheet = page.getByTestId('bottom-sheet');
    expect(
      await sheet.evaluate((element) => ({
        left: getComputedStyle(element).paddingLeft,
        right: getComputedStyle(element).paddingRight,
      })),
    ).toEqual({ left: '0px', right: '84px' });
  });

  test('the stage is counted from the room that is left, not from the screen', async ({ page }) => {
    // 640 wide is medium; with 84 kept at the right there are 556, and that is compact.
    await page.setViewportSize({ width: 640, height: 800 });
    await show(page, 'kanban', 'a');
    await expectStage(page, 'medium');
    await show(page, 'kanban', 'a', '&safe=right');
    await expectStage(page, 'compact');
    await expect(page.getByTestId('column-tabs')).toBeHidden();
    expect(await widthOf(page, 'pane')).toBe(556);
  });

  test('where a finger is the pointer the "…" of a card is always there, at any width', async ({
    page,
  }) => {
    for (const width of [360, 820] as const) {
      await page.setViewportSize(SIZES[width]);
      await show(page, 'kanban', 'a');
      await expect(
        page.locator('[data-card="c1"]').getByRole('button', { name: '카드 메뉴' }),
      ).toHaveCSS('opacity', '1');
    }
  });
});
