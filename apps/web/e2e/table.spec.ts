/**
 * The table view, end to end, on the in-memory backend. Every test runs at
 * the three stages (the projects `desktop`, `middle`, `mobile`) unless it
 * says otherwise: a mouse is needed to drag with, and only `desktop` has one.
 *
 * The tests find boards, views, rows and columns by what they are called and
 * read what was stored through the development handle (`window.fvociDev`),
 * so they do not depend on how the sidebar or the header of a view is drawn.
 */
import { scrollTo } from './scroll.support.ts';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { clipboardPermissions, primaryKey, primaryName, tabKey } from './keyboard.support.ts';
import { boardPath } from './address.ts';
import { screensDir } from './screens.ts';
import { touchScreen } from './touch.support.ts';

interface StoredView {
  id: string;
  title: string;
  fields: {
    visiblePropertyIds: string[];
    columnWidths: Record<string, number>;
    columnCalculations: Record<string, string>;
    collapsedOptionIds: string[];
    sortOptions: { propertyId: string; reversed: boolean }[];
    cardOrder: string[];
    groupById?: string;
  };
}

interface StoredCard {
  id: string;
  title: string;
  fields: { isTemplate: boolean; properties: Record<string, unknown> };
}

interface Dev {
  backend: {
    latency: number;
    workspaces(): Promise<{ id: string }[]>;
    boards(workspaceId: string): Promise<{ id: string; title: string }[]>;
    views(boardId: string): Promise<StoredView[]>;
    cards(boardId: string): Promise<StoredCard[]>;
    createCard(boardId: string, draft: { title: string }): Promise<StoredCard>;
  };
}

interface Opened {
  boardId: string;
  viewId: string;
}

const TITLE = '__title';
const STAGES: Record<string, string> = { desktop: 'expanded', middle: 'medium', mobile: 'compact' };

test.use({
  permissions: async ({ browserName }, use) => {
    await use(clipboardPermissions(browserName));
  },
});

/** Opens a view of a seeded board by the titles of both. */
async function openView(page: Page, boardTitle: string, viewTitle: string): Promise<Opened> {
  await page.goto('/');
  await expect(page.getByTestId('app-shell')).toBeVisible();
  const found = await page.evaluate(
    async ([board, view]) => {
      const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
      const [workspace] = await backend.workspaces();
      const boards = await backend.boards(workspace?.id ?? '');
      const boardId = boards.find((each) => each.title === board)?.id ?? '';
      const views = boardId === '' ? [] : await backend.views(boardId);
      return { boardId, viewId: views.find((each) => each.title === view)?.id ?? '' };
    },
    [boardTitle, viewTitle],
  );
  expect(found.boardId, `board "${boardTitle}"`).not.toBe('');
  expect(found.viewId, `view "${viewTitle}"`).not.toBe('');
  await page.goto(boardPath(found.boardId, found.viewId));
  await expect(page.getByTestId('table')).toBeVisible();
  return found;
}

/** The view as the backend has it now. */
async function storedView(page: Page, opened: Opened): Promise<StoredView['fields']> {
  return page.evaluate(async ({ boardId, viewId }) => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const view = (await backend.views(boardId)).find((each) => each.id === viewId);
    if (!view) throw new Error('the view is gone');
    return view.fields;
  }, opened);
}

async function storedCards(page: Page, opened: Opened): Promise<StoredCard[]> {
  return page.evaluate(async ({ boardId }) => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    return (await backend.cards(boardId)).filter((card) => !card.fields.isTemplate);
  }, opened);
}

/** Adds cards as somebody else would: they arrive as changes of the board. */
async function addCards(page: Page, opened: Opened, count: number): Promise<void> {
  await page.evaluate(
    async ({ boardId, count: wanted }) => {
      const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
      const latency = backend.latency;
      backend.latency = 0;
      for (let at = 0; at < wanted; at += 1) {
        await backend.createCard(boardId, { title: `Extra ${String(at + 1).padStart(3, '0')}` });
      }
      backend.latency = latency;
    },
    { ...opened, count },
  );
}

const table = (page: Page): Locator => page.getByTestId('table');
const cardRows = (page: Page): Locator =>
  table(page).locator('[data-nav-row]:has([data-kind="title"])');
const header = (page: Page, name: string): Locator =>
  table(page).getByRole('columnheader', { name, exact: true });
const rowOf = (page: Page, rowId: string): Locator =>
  table(page).locator(`[data-nav-row][data-row="${rowId}"]`);
const titleCell = (page: Page, rowId: string): Locator =>
  rowOf(page, rowId).locator('[data-kind="title"]');
const cellOf = (page: Page, rowId: string, columnId: string): Locator =>
  rowOf(page, rowId).locator(`[data-cell][data-col="${columnId}"]`);
/** A menu or a picker, whichever way it opened: a dropdown, a popover, or a sheet. */
const surface = (page: Page): Locator =>
  page.locator('[data-testid="dropdown"], [data-testid="bottom-sheet"], [data-presentation]');
const menuItem = (page: Page, action: string): Locator => page.locator(`[data-action="${action}"]`);

async function columnId(page: Page, name: string): Promise<string> {
  const id = await header(page, name).getAttribute('data-col');
  expect(id, `column "${name}"`).not.toBeNull();
  return id ?? '';
}

/** The titles of the rows, from top to bottom. */
async function rowTitles(page: Page): Promise<string[]> {
  return table(page)
    .locator('[data-kind="title"]')
    .evaluateAll((cells) =>
      cells.map(
        (cell) =>
          cell.querySelector('[data-slot="row-title"]')?.textContent ??
          cell.querySelector('input')?.value ??
          '',
      ),
    );
}

async function rowId(page: Page, title: string): Promise<string> {
  const id = await table(page)
    .locator('[data-slot="row-title"]')
    .evaluateAll(
      (titles, wanted) =>
        titles
          .find((each) => each.textContent === wanted)
          ?.closest('[data-row]')
          ?.getAttribute('data-row') ?? '',
      title,
    );
  expect(id, `row "${title}"`).not.toBe('');
  return id;
}

/** Scrolls a cell out from under the pinned row and column, as a person would before pressing it. */
async function reveal(cell: Locator): Promise<void> {
  await cell.evaluate((element) => {
    const root = element.closest<HTMLElement>('[data-testid="table"]');
    if (!root) return;
    const box = (selector: string): DOMRect | undefined =>
      root.querySelector(selector)?.getBoundingClientRect();
    const pinned = box('[data-sticky="top"] [data-pinned]');
    if (pinned && !element.hasAttribute('data-pinned')) {
      root.scrollLeft += element.getBoundingClientRect().left - pinned.right;
    }
    const top = box('[data-sticky="top"]');
    const bottom = box('[data-sticky="bottom"]');
    const rect = element.getBoundingClientRect();
    if (top && rect.top < top.bottom) root.scrollTop -= top.bottom - rect.top;
    else if (bottom && rect.bottom > bottom.top) root.scrollTop += rect.bottom - bottom.top;
  });
}

/** Presses a cell near its top left corner: the part that is in view even in a narrow pane. */
async function press(cell: Locator): Promise<void> {
  const page = cell.page();
  // A menu or a picker on its way out still covers the page, and keeps it from being pressed.
  await expect(surface(page)).toHaveCount(0);
  await expect
    .poll(() => page.evaluate(() => getComputedStyle(document.body).pointerEvents))
    .not.toBe('none');
  await reveal(cell);
  // At the point itself: a click on the locator would first scroll the cell to where it likes,
  // which in a narrow pane can be under the title column.
  const corner = await cell.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { x: rect.x, y: rect.y };
  });
  await page.mouse.click(corner.x + 10, corner.y + 12);
}

/** Opens the menu of a column's heading. */
async function openHeaderMenu(page: Page, name: string): Promise<void> {
  await press(header(page, name));
  await expect(surface(page).first()).toBeVisible();
}

/** Opens the "more" menu of a row. Where the device hovers, the button shows on hover. */
async function openRowMenu(page: Page, id: string): Promise<void> {
  await titleCell(page, id).hover();
  await rowOf(page, id).locator('[data-more]').click();
  await expect(surface(page).first()).toBeVisible();
}

/** Ctrl+Z with the focus in the table, on a cell. */
async function undo(page: Page): Promise<void> {
  // A change can be undone once the backend has answered it; the in-memory one takes 40 ms.
  await page.waitForTimeout(150);
  await table(page).locator('[data-cell][tabindex="0"]').first().focus();
  await page.keyboard.press(await primaryKey(page, 'z'));
}

async function scrollPlace(page: Page): Promise<{ x: number; y: number }> {
  return table(page).evaluate((element) => ({ x: element.scrollLeft, y: element.scrollTop }));
}

async function centre(locator: Locator): Promise<{ x: number; y: number; box: DOMRect }> {
  const box = await locator.evaluate(
    (element) => element.getBoundingClientRect().toJSON() as DOMRect,
  );
  return { x: box.x + box.width / 2, y: box.y + box.height / 2, box };
}

// ------------------------------------------------------------ what it shows

test.describe('what the table shows', () => {
  test('the columns of the view in its order, a row per card, and their values', async ({
    page,
  }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const fields = await storedView(page, opened);

    await expect(table(page)).toHaveAttribute('role', 'grid');
    const shown = await table(page)
      .locator('[data-kind="header"]')
      .evaluateAll((cells) => cells.map((cell) => cell.getAttribute('data-col')));
    expect(shown).toEqual([TITLE, ...fields.visiblePropertyIds]);
    await expect(header(page, 'Name')).toBeVisible();
    for (const name of ['Status', 'Priority', 'Estimated Hours']) {
      await expect(header(page, name)).toHaveCount(1);
    }

    // The view has no sort: the rows stand in its manual order.
    const cards = await storedCards(page, opened);
    const byId = new Map(cards.map((card) => [card.id, card.title]));
    expect(await rowTitles(page)).toEqual(fields.cardOrder.map((id) => byId.get(id)));

    const id = await rowId(page, 'Identify dependencies');
    await expect(cellOf(page, id, await columnId(page, 'Status'))).toHaveText('Not Started');
    await expect(cellOf(page, id, await columnId(page, 'Priority'))).toContainText('3. Low');
  });

  test('the title column, the heading row and the summary row stay put while the rest scrolls', async ({
    page,
  }) => {
    const opened = await openView(page, 'Sales Pipeline CRM', 'All Contacts');
    await addCards(page, opened, 40);
    await expect(cardRows(page)).toHaveCount(45);

    const before = await table(page).evaluate((element) => ({
      wide: element.scrollWidth - element.clientWidth,
      tall: element.scrollHeight - element.clientHeight,
    }));
    expect(before.wide).toBeGreaterThan(200);
    expect(before.tall).toBeGreaterThan(300);
    await expect(table(page)).not.toHaveAttribute('data-scrolled');

    await table(page).evaluate((element) => {
      element.scrollLeft = 200;
      element.scrollTop = 300;
    });
    await expect(table(page)).toHaveAttribute('data-scrolled', '');

    const places = await table(page).evaluate((element) => {
      const box = element.getBoundingClientRect();
      const of = (selector: string): DOMRect | undefined =>
        element.querySelector(selector)?.getBoundingClientRect();
      const rowTitle = [...element.querySelectorAll('[data-kind="title"]')]
        .map((cell) => cell.getBoundingClientRect())
        .find((rect) => rect.top > box.top + 100);
      const shadow = element.querySelector('[data-kind="title"]');
      return {
        left: box.left,
        top: box.top,
        bottom: box.top + element.clientHeight,
        headingTitle: of('[data-sticky="top"] [data-pinned]'),
        heading: of('[data-sticky="top"]'),
        summary: of('[data-sticky="bottom"]'),
        summaryTitle: of('[data-sticky="bottom"] [data-pinned]'),
        rowTitle,
        shadow: shadow ? getComputedStyle(shadow, '::after').opacity : '',
      };
    });
    // x = 0 of the table, wherever the shell puts the table on the page.
    expect(places.rowTitle?.left).toBeCloseTo(places.left, 0);
    expect(places.headingTitle?.left).toBeCloseTo(places.left, 0);
    expect(places.summaryTitle?.left).toBeCloseTo(places.left, 0);
    expect(places.heading?.top).toBeCloseTo(places.top, 0);
    expect(places.summary?.bottom).toBeCloseTo(places.bottom, 0);
    await expect
      .poll(() =>
        table(page)
          .locator('[data-kind="title"]')
          .first()
          .evaluate((cell) => getComputedStyle(cell, '::after').opacity),
      )
      .toBe('1');

    // The other columns pass under the title column: the first of them starts left of its right edge.
    const status = await header(page, 'Status').evaluate(
      (cell) => cell.getBoundingClientRect().left,
    );
    expect(status).toBeLessThan(places.headingTitle?.right ?? 0);
  });

  test('the title column is as wide as the reader made it, but never more than half of the pane', async ({
    page,
  }) => {
    const opened = await openView(page, 'Company Goals & OKRs', 'By Quarter');
    const stored = (await storedView(page, opened)).columnWidths[TITLE] ?? 0;
    expect(stored).toBe(452);
    const sizes = await table(page).evaluate((element) => ({
      pane: element.clientWidth,
      title: element.querySelector('[data-sticky="top"] [data-pinned]')?.getBoundingClientRect()
        .width,
    }));
    expect(sizes.title).toBeCloseTo(Math.min(stored, sizes.pane / 2), 0);
  });

  test('without a width of its own the title column follows the pane: 160, 224 or 352 pixels', async ({
    page,
  }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    // Take the reader's width away, as a view that never had one.
    await page.evaluate(async ({ boardId, viewId }) => {
      const dev = (window as unknown as { fvociDev: { backend: Record<string, unknown> } })
        .fvociDev;
      const patch = dev.backend.patchView as (
        board: string,
        view: string,
        patch: unknown,
      ) => Promise<unknown>;
      await patch.call(dev.backend, boardId, viewId, {
        updatedFields: { columnWidths: {} },
      });
    }, opened);
    for (const [pane, expected] of [
      [300, 150],
      [400, 160],
      [600, 224],
      [1100, 352],
    ] as const) {
      // The pane is given its width directly: what the shell leaves of the window is its business.
      await table(page).evaluate((element, width) => {
        const container = element.closest<HTMLElement>('main') ?? element.parentElement;
        if (!container) return;
        container.style.flex = 'none';
        container.style.width = `${width}px`;
      }, pane);
      await expect
        .poll(() =>
          table(page).evaluate(
            (element) =>
              element.querySelector('[data-sticky="top"] [data-pinned]')?.getBoundingClientRect()
                .width,
          ),
        )
        .toBe(expected);
    }
  });
});

// ------------------------------------------------------------------ cells

test.describe('editing a cell', () => {
  test('a text: pressing the cell gives a field, Enter stores it, and undo takes it back', async ({
    page,
  }) => {
    const opened = await openView(page, 'Sales Pipeline CRM', 'All Contacts');
    const id = await rowId(page, 'Richard Guzman');
    const company = cellOf(page, id, await columnId(page, 'Company'));
    await expect(company).toHaveText('Afformance Ltd.');
    // Nothing is an editor until a cell is pressed.
    await expect(table(page).locator('input')).toHaveCount(0);

    await press(company);
    const field = company.locator('input');
    await expect(field).toBeFocused();
    await expect(table(page).locator('input')).toHaveCount(1);
    await field.fill('Northwind');
    await page.keyboard.press('Enter');

    await expect(company).toHaveText('Northwind');
    await expect(table(page).locator('input')).toHaveCount(0);
    await expect(company).toBeFocused();
    await expect
      .poll(async () => (await storedCards(page, opened)).find((card) => card.id === id)?.fields)
      .toMatchObject({ properties: { [await columnId(page, 'Company')]: 'Northwind' } });

    await page.keyboard.press(await primaryKey(page, 'z'));
    await expect(company).toHaveText('Afformance Ltd.');
  });

  test('a text: Escape gives up what was typed', async ({ page }) => {
    await openView(page, 'Sales Pipeline CRM', 'All Contacts');
    const id = await rowId(page, 'Richard Guzman');
    const company = cellOf(page, id, await columnId(page, 'Company'));
    await press(company);
    await company.locator('input').fill('Never stored');
    await page.keyboard.press('Escape');
    await expect(company).toHaveText('Afformance Ltd.');
    await expect(company).toBeFocused();
  });

  test('a select: pressing the cell opens the options, choosing one stores it, and undo takes it back', async ({
    page,
  }, testInfo) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Identify dependencies');
    const priority = cellOf(page, id, await columnId(page, 'Priority'));
    await expect(priority).toContainText('3. Low');

    await press(priority);
    // A sheet on a compact page, a popover elsewhere: lib/layout/presentation.ts.
    const expected = testInfo.project.name === 'mobile' ? 'sheet' : 'popover';
    const picker = page.locator(`[data-presentation="${expected}"]`);
    await expect(picker).toBeVisible();
    await picker.getByText('2. Medium').click();

    await expect(picker).toBeHidden();
    await expect(priority).toContainText('2. Medium');
    // The cell is back to showing: no trigger of a picker is left in it.
    await expect(priority.locator('button')).toHaveCount(0);

    await undo(page);
    await expect(priority).toContainText('3. Low');
  });

  test('a checkbox: pressing the cell ticks it at once, and undo takes it back', async ({
    page,
  }) => {
    await openView(page, 'Personal Tasks', 'List View');
    const id = await rowId(page, 'Buy groceries');
    const done = cellOf(page, id, await columnId(page, 'Completed'));
    const box = done.getByRole('checkbox');
    const before = await box.isChecked();

    await press(done);
    await expect(box).toBeChecked({ checked: !before });
    await undo(page);
    await expect(box).toBeChecked({ checked: before });
  });

  test('the title: edited in place', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Define project scope');
    await titleCell(page, id).click({ position: { x: 6, y: 12 } });
    const field = titleCell(page, id).locator('input');
    await expect(field).toBeFocused();
    await expect(field).toHaveValue('Define project scope');
    await field.fill('Define the scope');
    await page.keyboard.press('Enter');

    await expect(titleCell(page, id).locator('[data-slot="row-title"]')).toHaveText(
      'Define the scope',
    );
    await expect
      .poll(async () => (await storedCards(page, opened)).find((card) => card.id === id)?.title)
      .toBe('Define the scope');
    await page.keyboard.press(await primaryKey(page, 'z'));
    await expect(titleCell(page, id).locator('[data-slot="row-title"]')).toHaveText(
      'Define project scope',
    );
  });

  test('a value that comes from the card itself is not edited', async ({ page }) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Define project scope');
    const created = cellOf(page, id, await columnId(page, 'Date Created'));
    await press(created);
    await expect(table(page).locator('input')).toHaveCount(0);
    await expect(page.locator('[data-presentation]')).toHaveCount(0);
  });
});

// ------------------------------------------------------------------- rows

test.describe('rows', () => {
  test('"+ New" makes a row and puts the caret in its title', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    await expect(cardRows(page)).toHaveCount(5);
    await table(page).getByRole('button', { name: '+ New' }).click();

    await expect(cardRows(page)).toHaveCount(6);
    const field = cardRows(page).last().locator('[data-kind="title"] input');
    await expect(field).toBeFocused();
    await field.fill('Write the report');
    await page.keyboard.press('Enter');
    expect((await rowTitles(page)).at(-1)).toBe('Write the report');
    await expect
      .poll(async () => (await storedCards(page, opened)).map((card) => card.title))
      .toContain('Write the report');
    // The summary row counts it.
    await expect(
      table(page).locator(`[data-kind="calculation"][data-col="${TITLE}"]`),
    ).toContainText('6');
  });

  test('the menu of a row deletes it, and undo brings it back', async ({ page }) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Conduct market analysis');
    await openRowMenu(page, id);
    await menuItem(page, 'delete').click();
    await expect(rowOf(page, id)).toHaveCount(0);
    await expect(cardRows(page)).toHaveCount(4);

    await undo(page);
    await expect(rowOf(page, id)).toHaveCount(1);
    await expect(cardRows(page)).toHaveCount(5);
  });

  test('the menu of a row makes a copy of it', async ({ page }) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Conduct market analysis');
    await openRowMenu(page, id);
    await menuItem(page, 'duplicate').click();
    await expect(cardRows(page)).toHaveCount(6);
    const titles = await rowTitles(page);
    expect(titles.filter((title) => title === 'Conduct market analysis')).toHaveLength(2);
  });

  test('the menu of a row copies the address of its card', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Conduct market analysis');
    await openRowMenu(page, id);
    await menuItem(page, 'copyLink').click();
    await expect(page.getByTestId('table-status')).toHaveText('Copied!');
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(new URL(copied).pathname).toBe(boardPath(opened.boardId, opened.viewId, id));
  });

  test('a row opens its card: by "Open" where the device hovers, and from its menu', async ({
    page,
  }, testInfo) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Conduct market analysis');
    const open = rowOf(page, id).locator('a[data-open-card]');

    if (testInfo.project.name === 'desktop') {
      // Hidden until the pointer is over the row.
      await expect(open).toHaveCSS('opacity', '1');
      const group = open.locator('xpath=..');
      await expect(group).toHaveCSS('opacity', '0');
      await titleCell(page, id).hover();
      await expect(group).toHaveCSS('opacity', '1');
      await open.click();
      await expect(page.getByTestId('card-dialog')).toBeVisible();
      await expect(page).toHaveURL(new RegExp(`/${id}$`));
      await page.goBack();
      await expect(page.getByTestId('card-dialog')).toBeHidden();
    } else {
      // No hover: the button of the menu is always there, and the menu opens the card.
      await expect(open).toBeHidden();
      await expect(rowOf(page, id).locator('[data-more]')).toBeVisible();
    }

    await openRowMenu(page, id);
    await menuItem(page, 'open').click();
    await expect(page.getByTestId('card-dialog')).toBeVisible();
    await expect(page).toHaveURL(new RegExp(`/${id}$`));
  });

  test('a finger gets room: rows and their buttons are 44 pixels where the pointer is coarse', async ({
    page,
  }, testInfo) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Conduct market analysis');
    const row = await rowOf(page, id).evaluate((element) => element.getBoundingClientRect().height);
    if (testInfo.project.name === 'desktop') {
      expect(row).toBeGreaterThanOrEqual(36);
      return;
    }
    expect(row).toBeGreaterThanOrEqual(44);
    const more = await rowOf(page, id)
      .locator('[data-more]')
      .evaluate((element) => element.getBoundingClientRect());
    expect(more.width).toBeGreaterThanOrEqual(44);
    expect(more.height).toBeGreaterThanOrEqual(44);
  });
});

// ---------------------------------------------------------------- columns

test.describe('columns', () => {
  test('the menu of a heading makes the column wider and narrower, and the view keeps it', async ({
    page,
  }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const status = await columnId(page, 'Status');
    const width = (): Promise<number> =>
      header(page, 'Status').evaluate((cell) => cell.getBoundingClientRect().width);
    expect(await width()).toBe(141);

    await openHeaderMenu(page, 'Status');
    await menuItem(page, 'wider').click();
    await expect.poll(width).toBe(189);
    await expect.poll(async () => (await storedView(page, opened)).columnWidths[status]).toBe(189);

    await openHeaderMenu(page, 'Status');
    await menuItem(page, 'narrower').click();
    await openHeaderMenu(page, 'Status');
    await menuItem(page, 'narrower').click();
    // Never narrower than the least width.
    await expect.poll(width).toBe(100);
    await expect.poll(async () => (await storedView(page, opened)).columnWidths[status]).toBe(100);
    // Every row follows.
    const id = await rowId(page, 'Define project scope');
    expect(
      await cellOf(page, id, status).evaluate((cell) => cell.getBoundingClientRect().width),
    ).toBe(100);

    await undo(page);
    await expect.poll(width).toBe(141);
  });

  test('the handle at the edge of a heading is moved with the arrow keys', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const status = await columnId(page, 'Status');
    const handle = table(page).locator(`[data-resize="${status}"]`);
    await expect(handle).toHaveAttribute('role', 'separator');
    await expect(handle).toHaveAttribute('aria-valuenow', '141');
    await handle.focus();
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    await expect(handle).toHaveAttribute('aria-valuenow', '173');
    await page.keyboard.press('ArrowLeft');
    await expect(handle).toHaveAttribute('aria-valuenow', '157');
    await expect.poll(async () => (await storedView(page, opened)).columnWidths[status]).toBe(157);
    // The arrows were the handle's: the focus did not move to another cell.
    await expect(handle).toBeFocused();
  });

  test('the handle is dragged with the mouse; the width is stored once, when it is let go', async ({
    page,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'dragging the handle needs a mouse');
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const status = await columnId(page, 'Status');
    const grip = await centre(table(page).locator(`[data-resize="${status}"]`));
    const width = (): Promise<number> =>
      header(page, 'Status').evaluate((cell) => cell.getBoundingClientRect().width);

    await page.mouse.move(grip.x, grip.y);
    await page.mouse.down();
    await page.mouse.move(grip.x + 30, grip.y, { steps: 3 });
    await page.mouse.move(grip.x + 60, grip.y, { steps: 3 });
    // The table follows while it is dragged, and nothing is stored yet.
    await expect.poll(width).toBe(201);
    expect((await storedView(page, opened)).columnWidths[status]).toBe(141);
    await page.mouse.up();
    await expect.poll(async () => (await storedView(page, opened)).columnWidths[status]).toBe(201);
    await expect.poll(width).toBe(201);
    // Letting go of the handle did not open the menu of the heading.
    await expect(surface(page)).toHaveCount(0);

    // One change, one undo.
    await undo(page);
    await expect.poll(width).toBe(141);
  });

  test('the width of the title column changes from its menu too', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const shown = (): Promise<number> =>
      header(page, 'Name').evaluate((cell) => Math.round(cell.getBoundingClientRect().width));
    const before = await shown();
    await openHeaderMenu(page, 'Name');
    await menuItem(page, 'narrower').click();
    // From the width it shows (half of the pane where that is less than the 280 pixels stored),
    // and what it shows is again never more than half of the pane.
    const half = await table(page).evaluate((element) => element.clientWidth / 2);
    await expect.poll(shown).toBe(Math.round(Math.min(Math.max(100, before - 48), half)));
    await expect
      .poll(async () => (await storedView(page, opened)).columnWidths[TITLE])
      .toBe(Math.max(100, before - 48));
  });

  test('the menu of a heading moves the column a step, and hides it', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const before = (await storedView(page, opened)).visiblePropertyIds;
    const status = await columnId(page, 'Status');
    expect(before[0]).toBe(status);

    await openHeaderMenu(page, 'Status');
    await expect(menuItem(page, 'moveLeft')).toBeDisabled();
    await menuItem(page, 'moveRight').click();
    const moved = [before[1], before[0], ...before.slice(2)];
    await expect
      .poll(async () => (await storedView(page, opened)).visiblePropertyIds)
      .toEqual(moved);
    const shown = await table(page)
      .locator('[data-kind="header"]')
      .evaluateAll((cells) => cells.map((cell) => cell.getAttribute('data-col')));
    expect(shown).toEqual([TITLE, ...moved]);
    // The cells of every row moved with the heading.
    const id = await rowId(page, 'Define project scope');
    const cells = await rowOf(page, id)
      .locator('[data-cell]')
      .evaluateAll((all) => all.map((cell) => cell.getAttribute('data-col')));
    expect(cells).toEqual([TITLE, ...moved]);

    await openHeaderMenu(page, 'Status');
    await menuItem(page, 'hide').click();
    await expect(header(page, 'Status')).toHaveCount(0);
    await expect
      .poll(async () => (await storedView(page, opened)).visiblePropertyIds)
      .toEqual(moved.filter((each) => each !== status));

    await undo(page);
    await expect(header(page, 'Status')).toHaveCount(1);
  });

  test('a heading is dragged to another place with the mouse', async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'dragging a heading needs a mouse');
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const before = (await storedView(page, opened)).visiblePropertyIds;
    const from = await centre(header(page, 'Status'));
    const to = await centre(header(page, 'Date Created'));

    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(from.x + 8, from.y, { steps: 2 });
    await page.mouse.move(to.x + 20, to.y, { steps: 10 });
    await expect(table(page)).toHaveAttribute('data-dragging', 'pointer');
    // A line shows where it would land: between "Date Created" and "Due Date".
    await expect(header(page, 'Due Date')).toHaveAttribute('data-drop', 'before');
    await expect(page.locator('[data-drag-overlay]')).toBeVisible();
    await page.mouse.up();

    const expected = [before[1], before[2], before[0], ...before.slice(3)];
    await expect
      .poll(async () => (await storedView(page, opened)).visiblePropertyIds)
      .toEqual(expected);
    // The drag was not a click: the menu of the heading did not open.
    await expect(surface(page)).toHaveCount(0);
  });

  test('a heading is moved with the keyboard: Space, arrows, Space', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const before = (await storedView(page, opened)).visiblePropertyIds;
    await header(page, 'Status').focus();
    await page.keyboard.press('Space');
    await expect(table(page)).toHaveAttribute('data-dragging', 'keyboard');
    await expect(page.getByTestId('table-column-announcer')).toContainText(
      'Picked up the Status column',
    );
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    await expect(page.getByTestId('table-column-announcer')).toContainText('column 3 of 7');
    await page.keyboard.press('Space');
    await expect(table(page)).not.toHaveAttribute('data-dragging');

    const expected = [before[1], before[2], before[0], ...before.slice(3)];
    await expect
      .poll(async () => (await storedView(page, opened)).visiblePropertyIds)
      .toEqual(expected);
    await expect(header(page, 'Status')).toBeFocused();

    // Escape puts a carried column back.
    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('Escape');
    await expect(table(page)).not.toHaveAttribute('data-dragging');
    expect((await storedView(page, opened)).visiblePropertyIds).toEqual(expected);
  });

  test('the menu of a heading sorts the view by its column', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    await openHeaderMenu(page, 'Name');
    await menuItem(page, 'sortDescending').click();
    await expect(header(page, 'Name')).toHaveAttribute('aria-sort', 'descending');
    await expect
      .poll(async () => (await storedView(page, opened)).sortOptions)
      .toEqual([{ propertyId: TITLE, reversed: true }]);
    const descending = await rowTitles(page);
    expect(descending).toEqual([...descending].sort((a, b) => b.localeCompare(a, 'en')));

    await openHeaderMenu(page, 'Name');
    await menuItem(page, 'sortAscending').click();
    await expect(header(page, 'Name')).toHaveAttribute('aria-sort', 'ascending');
    await expect.poll(() => rowTitles(page)).toEqual([...descending].reverse());
  });
});

// ----------------------------------------------------------------- groups

test.describe('groups', () => {
  test('a heading per group with its count; folding hides its rows and the view keeps that', async ({
    page,
  }) => {
    const opened = await openView(page, 'Sprint Planner', 'By Sprint');
    const groups = table(page).locator('[data-group-header]');
    await expect(groups).not.toHaveCount(0);
    const counts = await groups.evaluateAll((all) =>
      all.map((group) =>
        Number(group.querySelector('[data-slot="group-count"] [aria-hidden]')?.textContent),
      ),
    );
    expect(counts.reduce((sum, count) => sum + count, 0)).toBe(await cardRows(page).count());
    // The group of cards without a value is hidden in this view, and so not drawn.
    await expect(table(page).locator('[data-row="group:"]')).toHaveCount(0);

    const first = groups.first();
    const optionId = ((await first.getAttribute('data-row')) ?? '').replace('group:', '');
    const inFirst = table(page).locator(`[data-list="group:${optionId}"] [data-kind="title"]`);
    const size = counts[0] ?? 0;
    expect(size).toBeGreaterThan(0);
    await expect(inFirst).toHaveCount(size);

    const name = (await first.locator('[data-slot="group-name"]').textContent()) ?? '';
    await first.getByRole('button', { name: `Collapse ${name.trim()}` }).click();
    await expect(inFirst).toHaveCount(0);
    // The heading still says how many there are.
    await expect(first.locator('[data-slot="group-count"] [aria-hidden]')).toHaveText(String(size));
    await expect
      .poll(async () => (await storedView(page, opened)).collapsedOptionIds)
      .toEqual([optionId]);

    await first.getByRole('button', { name: `Expand ${name.trim()}` }).click();
    await expect(inFirst).toHaveCount(size);
    await expect.poll(async () => (await storedView(page, opened)).collapsedOptionIds).toEqual([]);
  });

  test('"+" on a group makes a card with the value of the group', async ({ page }) => {
    const opened = await openView(page, 'Sprint Planner', 'By Sprint');
    const fields = await storedView(page, opened);
    const group = table(page).locator('[data-group-header]').nth(1);
    const optionId = ((await group.getAttribute('data-row')) ?? '').replace('group:', '');
    const inGroup = table(page).locator(`[data-list="group:${optionId}"] [data-kind="title"]`);
    const size = await inGroup.count();

    await group.locator('[data-group-add]').click();
    await expect(inGroup).toHaveCount(size + 1);
    const field = inGroup.last().locator('input');
    await expect(field).toBeFocused();
    await field.fill('Grouped card');
    await page.keyboard.press('Enter');

    await expect
      .poll(async () => {
        const card = (await storedCards(page, opened)).find(
          (each) => each.title === 'Grouped card',
        );
        return card?.fields.properties[fields.groupById ?? ''];
      })
      .toBe(optionId);
    // The calculation still counts every card, not those of one group.
    const total = await cardRows(page).count();
    await expect(
      table(page).locator(`[data-kind="calculation"][data-col="${TITLE}"]`),
    ).toContainText(String(total));
  });

  test('"+ New" at the foot of a group does the same', async ({ page }) => {
    const opened = await openView(page, 'Sprint Planner', 'By Sprint');
    const fields = await storedView(page, opened);
    const group = table(page).locator('[data-group-header]').first();
    const optionId = ((await group.getAttribute('data-row')) ?? '').replace('group:', '');
    await table(page)
      .locator(`[data-list="group:${optionId}"]`)
      .getByRole('button', { name: '+ New' })
      .click();
    const field = table(page).locator(`[data-list="group:${optionId}"] input`);
    await expect(field).toBeFocused();
    await field.fill('At the foot');
    await page.keyboard.press('Enter');
    await expect
      .poll(async () => {
        const card = (await storedCards(page, opened)).find((each) => each.title === 'At the foot');
        return card?.fields.properties[fields.groupById ?? ''];
      })
      .toBe(optionId);
  });
});

// ----------------------------------------------------------- calculations

test.describe('the summary row', () => {
  test('shows the calculation the view has for a column', async ({ page }) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const hours = await columnId(page, 'Estimated Hours');
    const cell = table(page).locator(`[data-kind="calculation"][data-col="${hours}"]`);
    await expect(cell).toHaveAttribute('data-calculation', 'sum');
    await expect(cell.locator('[data-slot="calculation-name"]')).toHaveText('Sum');
    const values = await table(page)
      .locator(`[data-kind="value"][data-col="${hours}"]`)
      .allTextContents();
    const sum = values.reduce((total, text) => total + (Number.parseFloat(text) || 0), 0);
    await expect(cell.locator('[data-slot="calculation-result"]')).toHaveText(String(sum));
    // The title column counts the cards unless the view says otherwise.
    await expect(table(page).locator(`[data-kind="calculation"][data-col="${TITLE}"]`)).toHaveText(
      /Count\s*5/,
    );
  });

  test('pressing a cell of it offers the calculations of the kind, and choosing one shows its result', async ({
    page,
  }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const hours = await columnId(page, 'Estimated Hours');
    const status = await columnId(page, 'Status');

    const statusCell = table(page).locator(`[data-kind="calculation"][data-col="${status}"]`);
    await press(statusCell);
    await expect(menuItem(page, 'countUniqueValue')).toBeVisible();
    // A select has no sum.
    await expect(menuItem(page, 'sum')).toHaveCount(0);
    await menuItem(page, 'countUniqueValue').click();
    await expect(statusCell).toHaveText(/Unique\s*2/);
    await expect
      .poll(async () => (await storedView(page, opened)).columnCalculations[status])
      .toBe('countUniqueValue');

    const hoursCell = table(page).locator(`[data-kind="calculation"][data-col="${hours}"]`);
    await press(hoursCell);
    await expect(menuItem(page, 'sum')).toBeVisible();
    await menuItem(page, 'max').click();
    await expect(hoursCell.locator('[data-slot="calculation-name"]')).toHaveText('Max');
    const values = await table(page)
      .locator(`[data-kind="value"][data-col="${hours}"]`)
      .allTextContents();
    const max = Math.max(...values.map((text) => Number.parseFloat(text) || 0));
    await expect(hoursCell.locator('[data-slot="calculation-result"]')).toHaveText(String(max));

    await undo(page);
    await expect(hoursCell.locator('[data-slot="calculation-name"]')).toHaveText('Sum');
  });

  test('a checkbox column can count its ticks', async ({ page }) => {
    await openView(page, 'Personal Tasks', 'List View');
    const done = await columnId(page, 'Completed');
    const cell = table(page).locator(`[data-kind="calculation"][data-col="${done}"]`);
    await press(cell);
    await menuItem(page, 'countChecked').click();
    const ticked = await table(page)
      .locator(`[data-kind="checkbox"][data-col="${done}"] [role="checkbox"][aria-checked="true"]`)
      .count();
    await expect(cell).toHaveText(new RegExp(`Checked\\s*${ticked}`));
  });
});

// --------------------------------------------------------------- dragging

test.describe('moving rows', () => {
  test('a row is dragged by its title with the mouse, to the place the line shows', async ({
    page,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'dragging a row needs a mouse');
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const titles = await rowTitles(page);
    const ids = await Promise.all(titles.map((title) => rowId(page, title)));
    const [first, , third, fourth] = ids;
    if (!first || !third || !fourth) throw new Error('the board has fewer rows than expected');

    const from = await centre(titleCell(page, first));
    const to = await centre(titleCell(page, third));
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(from.x + 2, from.y + 8, { steps: 2 });
    await page.mouse.move(to.x, to.y + to.box.height / 2 - 3, { steps: 10 });
    await expect(table(page)).toHaveAttribute('data-dragging', 'pointer');
    // Past the middle of the third row: it would land above the fourth.
    await expect(rowOf(page, fourth)).toHaveAttribute('data-drop-before', '');
    await expect(page.locator('[data-drag-overlay]')).toBeVisible();
    await page.mouse.up();

    const expected = [titles[1], titles[2], titles[0], ...titles.slice(3)];
    await expect.poll(() => rowTitles(page)).toEqual(expected);
    await expect
      .poll(async () => (await storedView(page, opened)).cardOrder.slice(0, 5))
      .toEqual([ids[1], ids[2], ids[0], ids[3], ids[4]]);
    // The drag was not a click: the title is not being edited.
    await expect(table(page).locator('input')).toHaveCount(0);

    await undo(page);
    await expect.poll(() => rowTitles(page)).toEqual(titles);
  });

  test('a finger that rests on the title picks the row up; moving it carries the row', async ({
    page,
  }, testInfo) => {
    test.skip(testInfo.project.name === 'desktop', 'a finger: the projects with a touch screen');
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const titles = await rowTitles(page);
    const ids = await Promise.all(titles.map((title) => rowId(page, title)));
    const [first, , third, fourth] = ids;
    if (!first || !third || !fourth) throw new Error('the board has fewer rows than expected');

    // Touch events as a touch screen sends them: the mouse of the test runner is not a finger.
    const client = await touchScreen(page);
    const touch = (type: 'touchStart' | 'touchMove' | 'touchEnd', at?: { x: number; y: number }) =>
      client.send('Input.dispatchTouchEvent', {
        type,
        touchPoints: at ? [{ x: Math.round(at.x), y: Math.round(at.y), id: 1 }] : [],
      });
    const from = await centre(titleCell(page, first));
    const to = await centre(titleCell(page, third));
    const start = { x: from.box.x + 14, y: from.y };
    const end = { x: start.x, y: to.y + to.box.height / 2 - 3 };

    // When the finger lands and when the table says the row is picked up, by
    // the page's own clock: a busy machine runs the test's steps late, the
    // page's timers never early.
    await table(page).evaluate((element) => {
      const times: { landed?: number; picked?: number } = {};
      Reflect.set(window, 'restTimes', times);
      document.addEventListener(
        'pointerdown',
        () => {
          // When the page handles the press, as the rest starts: not when the touch was made.
          times.landed ??= performance.now();
        },
        { capture: true },
      );
      new MutationObserver(() => {
        if (element.hasAttribute('data-dragging')) times.picked ??= performance.now();
      }).observe(element, { attributes: true, attributeFilter: ['data-dragging'] });
    });
    await touch('touchStart', start);
    await expect(table(page)).toHaveAttribute('data-dragging', 'pointer');
    // Not at once: a finger that has only just landed may still be about to
    // scroll. It rests 300ms (drag/settings.ts `touchDelay`) first.
    const rest = await page.evaluate(() => {
      const times = Reflect.get(window, 'restTimes') as { landed?: number; picked?: number };
      return (times.picked ?? 0) - (times.landed ?? Infinity);
    });
    expect(rest).toBeGreaterThanOrEqual(295);
    for (let step = 1; step <= 8; step += 1) {
      await touch('touchMove', { x: start.x, y: start.y + ((end.y - start.y) * step) / 8 });
      await page.waitForTimeout(20);
    }
    await expect(rowOf(page, fourth)).toHaveAttribute('data-drop-before', '');
    await touch('touchEnd');

    const expected = [titles[1], titles[2], titles[0], ...titles.slice(3)];
    await expect.poll(() => rowTitles(page)).toEqual(expected);
    await expect.poll(async () => (await storedView(page, opened)).cardOrder[2]).toBe(first);
    // The long press was not a tap: the title is not being edited.
    await expect(table(page).locator('input')).toHaveCount(0);
  });

  test('a row let go outside the table goes back', async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'dragging a row needs a mouse');
    await openView(page, 'Project Tasks', 'Task Overview');
    const titles = await rowTitles(page);
    const first = await rowId(page, titles[0] ?? '');
    const from = await centre(titleCell(page, first));
    const box = await centre(table(page));
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(from.x + 2, from.y + 8, { steps: 2 });
    await page.mouse.move(from.x, box.box.y - 20, { steps: 8 });
    await expect(page.getByTestId('table-notice')).toHaveText('Let go to cancel.');
    await page.mouse.up();
    await expect(table(page)).not.toHaveAttribute('data-dragging');
    expect(await rowTitles(page)).toEqual(titles);
  });

  test('a row is moved with the keyboard: Space on its title, arrows, Space', async ({ page }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    const titles = await rowTitles(page);
    const first = await rowId(page, titles[0] ?? '');

    await titleCell(page, first).focus();
    await page.keyboard.press('Space');
    await expect(table(page)).toHaveAttribute('data-dragging', 'keyboard');
    await expect(page.getByTestId('table-row-announcer')).toContainText(`Picked up ${titles[0]}`);
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await expect(page.getByTestId('table-row-announcer')).toContainText('row 3 of 5');
    // The row stands where it would land while it is carried.
    expect(await rowTitles(page)).toEqual([titles[1], titles[2], titles[0], ...titles.slice(3)]);
    await page.keyboard.press('Space');

    await expect(table(page)).not.toHaveAttribute('data-dragging');
    await expect
      .poll(() => rowTitles(page))
      .toEqual([titles[1], titles[2], titles[0], ...titles.slice(3)]);
    await expect.poll(async () => (await storedView(page, opened)).cardOrder[2]).toBe(first);
    await expect(titleCell(page, first)).toBeFocused();

    await page.keyboard.press('Space');
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Escape');
    expect(await rowTitles(page)).toEqual([titles[1], titles[2], titles[0], ...titles.slice(3)]);
  });

  test('a row carried into another group takes the value of that group', async ({ page }) => {
    const opened = await openView(page, 'Sprint Planner', 'By Sprint');
    const fields = await storedView(page, opened);
    const groups = await table(page)
      .locator('[data-group-header]')
      .evaluateAll((all) => all.map((group) => (group.getAttribute('data-row') ?? '').slice(6)));
    const [from, to] = groups;
    if (from === undefined || to === undefined)
      throw new Error('the view has fewer groups than expected');
    const moved = table(page).locator(`[data-list="group:${from}"] [data-kind="title"]`).first();
    const id = (await moved.locator('xpath=..').getAttribute('data-row')) ?? '';

    await moved.focus();
    await page.keyboard.press('Space');
    // Left and right go from group to group.
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('Space');

    await expect(table(page).locator(`[data-list="group:${to}"] [data-row="${id}"]`)).toHaveCount(
      1,
    );
    await expect
      .poll(async () => {
        const card = (await storedCards(page, opened)).find((each) => each.id === id);
        return card?.fields.properties[fields.groupById ?? ''];
      })
      .toBe(to);
    // The value and the place are one step back.
    await undo(page);
    await expect(table(page).locator(`[data-list="group:${from}"] [data-row="${id}"]`)).toHaveCount(
      1,
    );
  });

  test('in a sorted view rows are not moved by hand', async ({ page }) => {
    await openView(page, 'User Research Sessions', 'All Users');
    await expect(cardRows(page)).not.toHaveCount(0);
    await expect(table(page).locator('[data-nav-row][data-drag-item]')).toHaveCount(0);
    const first = cardRows(page).first().locator('[data-kind="title"]');
    await first.focus();
    await page.keyboard.press('Space');
    await expect(table(page)).not.toHaveAttribute('data-dragging');
  });
});

// -------------------------------------------------------------- read only

test.describe('a board the reader may only look at', () => {
  test('nothing that changes it is offered', async ({ page }) => {
    const opened = await openView(page, 'Company Goals & OKRs', 'By Quarter');
    await expect(table(page)).toHaveAttribute('aria-readonly', 'true');
    await expect(cardRows(page)).not.toHaveCount(0);

    await expect(table(page).getByRole('button', { name: '+ New' })).toHaveCount(0);
    await expect(table(page).locator('[data-group-add]')).toHaveCount(0);
    await expect(table(page).locator('[data-resize]')).toHaveCount(0);
    await expect(table(page).locator('[data-drag-item]')).toHaveCount(0);

    // A heading has no menu.
    await header(page, 'Name').click({ position: { x: 10, y: 12 } });
    await expect(surface(page)).toHaveCount(0);
    // A cell does not turn into an editor.
    const first = cardRows(page).first();
    const id = (await first.getAttribute('data-row')) ?? '';
    await press(cellOf(page, id, await columnId(page, 'Status')));
    await expect(page.locator('[data-presentation]')).toHaveCount(0);
    await titleCell(page, id).click({ position: { x: 6, y: 12 } });
    await expect(table(page).locator('input')).toHaveCount(0);
    // The summary row shows, and offers nothing.
    await table(page).locator(`[data-kind="calculation"][data-col="${TITLE}"]`).click();
    await expect(surface(page)).toHaveCount(0);

    // The menu of a row opens the card and copies its address, nothing more.
    await openRowMenu(page, id);
    await expect(menuItem(page, 'open')).toBeVisible();
    await expect(menuItem(page, 'copyLink')).toBeVisible();
    await expect(menuItem(page, 'delete')).toHaveCount(0);
    await expect(menuItem(page, 'duplicate')).toHaveCount(0);
    await page.keyboard.press('Escape');

    // Folding a group is not a change of the view: it works, and nothing is stored.
    const group = table(page).locator('[data-group-header]').first();
    const optionId = ((await group.getAttribute('data-row')) ?? '').replace('group:', '');
    const inGroup = table(page).locator(`[data-list="group:${optionId}"] [data-kind="title"]`);
    await expect(inGroup).not.toHaveCount(0);
    await group.getByRole('button', { name: /^Collapse / }).click();
    await expect(inGroup).toHaveCount(0);
    expect((await storedView(page, opened)).collapsedOptionIds).toEqual([]);
  });
});

// ------------------------------------------------------------- the window

test.describe('when the window changes size', () => {
  test('the place the table is scrolled to stays, across the three stages and back', async ({
    page,
  }) => {
    const opened = await openView(page, 'Sales Pipeline CRM', 'All Contacts');
    await addCards(page, opened, 60);
    await expect(cardRows(page)).toHaveCount(65);
    const start = page.viewportSize() ?? { width: 1280, height: 720 };
    const scroller = await table(page).elementHandle();

    await scrollTo(table(page), 180, 420);
    await expect.poll(() => scrollPlace(page)).toEqual({ x: 180, y: 420 });

    // Compact, medium, expanded, turned on its side, and back to where it began.
    for (const size of [
      { width: 412, height: 839 },
      { width: 626, height: 890 },
      { width: 890, height: 626 },
      { width: 1280, height: 720 },
      { width: 626, height: 890 },
      start,
    ]) {
      await page.setViewportSize(size);
      await expect
        .poll(() => scrollPlace(page), { message: `${size.width}×${size.height}` })
        .toEqual({
          x: 180,
          y: 420,
        });
    }
    // The same element all along: nothing was drawn anew.
    expect(await table(page).evaluate((element, before) => element === before, scroller)).toBe(
      true,
    );
  });

  test('a place the wider window cannot hold comes back when the window narrows again', async ({
    page,
  }) => {
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    await addCards(page, opened, 30);
    await page.setViewportSize({ width: 900, height: 500 });
    await expect(cardRows(page)).toHaveCount(35);
    const room = await table(page).evaluate((element) => ({
      x: element.scrollWidth - element.clientWidth,
      y: element.scrollHeight - element.clientHeight,
    }));
    expect(room.x).toBeGreaterThan(150);
    expect(room.y).toBeGreaterThan(200);
    const place = { x: Math.min(300, room.x), y: 200 };
    await scrollTo(table(page), place.x, place.y);
    await expect.poll(() => scrollPlace(page)).toEqual(place);

    // In a window this large the whole table fits: there is nothing to scroll.
    await page.setViewportSize({ width: 2400, height: 2000 });
    await expect.poll(() => scrollPlace(page)).toEqual({ x: 0, y: 0 });
    await page.setViewportSize({ width: 900, height: 500 });
    await expect.poll(() => scrollPlace(page)).toEqual(place);

    // What the reader scrolls to in the large window is theirs: it is not undone later.
    await page.setViewportSize({ width: 1400, height: 500 });
    await scrollTo(table(page), 0, 40);
    await expect.poll(() => scrollPlace(page)).toEqual({ x: 0, y: 40 });
    await page.setViewportSize({ width: 900, height: 500 });
    await expect.poll(() => scrollPlace(page)).toEqual({ x: 0, y: 40 });
  });

  test('a layout that is one beat late does not cost the sideways place', async ({ page }) => {
    // What docs/mobile-proposal.md found: the window widens, the pane is laid
    // out once at the width the old stage gives it (wide enough to cut the
    // scroll position), and only then a script narrows it to its real width.
    const opened = await openView(page, 'Project Tasks', 'Task Overview');
    await addCards(page, opened, 5);
    await page.setViewportSize({ width: 900, height: 600 });
    const room = await table(page).evaluate((element) => element.scrollWidth - element.clientWidth);
    expect(room).toBeGreaterThan(150);
    await scrollTo(table(page), 120, 0);
    await expect.poll(() => scrollPlace(page)).toMatchObject({ x: 120 });

    const cut = await table(page).evaluate(
      (element) =>
        new Promise<number>((resolve) => {
          const pane = element.closest<HTMLElement>('main') ?? element.parentElement;
          if (!pane) throw new Error('the table has no pane');
          let seen = -1;
          // As the page's own observer does: it sees the wide box after the
          // layout, and narrows it in answer.
          const observer = new ResizeObserver(() => {
            if (seen !== -1) return;
            seen = element.scrollLeft;
            pane.style.flex = '';
            pane.style.width = '';
            observer.disconnect();
            resolve(seen);
          });
          pane.style.flex = 'none';
          pane.style.width = '3000px';
          observer.observe(pane);
        }),
    );
    // The browser did cut the position in the layout in between.
    expect(cut).toBeLessThan(120);
    await expect.poll(() => scrollPlace(page)).toMatchObject({ x: 120 });
  });

  test('a field being typed in keeps its text and the caret', async ({ page }) => {
    await openView(page, 'Sales Pipeline CRM', 'All Contacts');
    const start = page.viewportSize() ?? { width: 1280, height: 720 };
    const id = await rowId(page, 'Richard Guzman');
    const company = cellOf(page, id, await columnId(page, 'Company'));
    await press(company);
    const field = company.locator('input');
    await field.fill('Half typ');
    for (const size of [{ width: 412, height: 839 }, { width: 890, height: 626 }, start]) {
      await page.setViewportSize(size);
      await expect(field).toBeFocused();
    }
    await page.keyboard.type('ed');
    await expect(field).toHaveValue('Half typed');
  });
});

// ------------------------------------------------------------ the keyboard

test.describe('with the keyboard alone', () => {
  test('the table is one stop of the Tab key', async ({ page }) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    await expect(table(page).locator('[data-cell][tabindex="0"]')).toHaveCount(1);
    const id = await rowId(page, 'Identify dependencies');
    await cellOf(page, id, await columnId(page, 'Status')).focus();
    await expect(table(page).locator('[data-cell][tabindex="0"]')).toHaveCount(1);
    await expect(cellOf(page, id, await columnId(page, 'Status'))).toHaveAttribute('tabindex', '0');
  });

  test('arrows move from cell to cell, Enter edits, Escape leaves, Space ticks', async ({
    page,
  }) => {
    const opened = await openView(page, 'Sales Pipeline CRM', 'All Contacts');
    const fields = await storedView(page, opened);
    const titles = await rowTitles(page);
    const ids = await Promise.all(titles.map((title) => rowId(page, title)));
    const company = await columnId(page, 'Company');
    const companyAt = fields.visiblePropertyIds.indexOf(company) + 1;

    // From the heading of the title column, down to the first card and along its row.
    await header(page, 'Name').focus();
    await page.keyboard.press('ArrowDown');
    await expect(titleCell(page, ids[0] ?? '')).toBeFocused();
    for (let step = 0; step < companyAt; step += 1) await page.keyboard.press('ArrowRight');
    const cell = cellOf(page, ids[0] ?? '', company);
    await expect(cell).toBeFocused();
    // The focused cell is in view, not under the title column.
    const clear = await cell.evaluate((element) => {
      const pinned = element
        .closest('[data-testid="table"]')
        ?.querySelector('[data-sticky="top"] [data-pinned]')
        ?.getBoundingClientRect();
      return element.getBoundingClientRect().left >= (pinned?.right ?? 0) - 1;
    });
    expect(clear).toBe(true);

    await page.keyboard.press('Enter');
    await expect(cell.locator('input')).toBeFocused();
    await page.keyboard.type(' (typed)');
    await page.keyboard.press('Escape');
    await expect(cell).toBeFocused();
    const before = (await cell.textContent()) ?? '';
    expect(before).not.toContain('(typed)');

    await page.keyboard.press('Enter');
    await page.keyboard.press(await primaryKey(page, 'a'));
    await page.keyboard.type('Keyboard Co.');
    await page.keyboard.press('Enter');
    await expect(cell).toHaveText('Keyboard Co.');
    await expect(cell).toBeFocused();

    // Down the column, and to the ends of the row.
    await page.keyboard.press('ArrowDown');
    await expect(cellOf(page, ids[1] ?? '', company)).toBeFocused();
    await page.keyboard.press('Home');
    await expect(titleCell(page, ids[1] ?? '')).toBeFocused();
    // Enter on the title edits it.
    await page.keyboard.press('Enter');
    await expect(titleCell(page, ids[1] ?? '').locator('input')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(titleCell(page, ids[1] ?? '')).toBeFocused();
    // The other platform's modifier cannot move the grid focus to its ends.
    const other = (await primaryName(page)) === 'Meta' ? 'Control' : 'Meta';
    await page.keyboard.press(`${other}+End`);
    await expect(titleCell(page, ids[1] ?? '')).toBeFocused();
    await page.keyboard.press(`${other}+Home`);
    await expect(titleCell(page, ids[1] ?? '')).toBeFocused();
    // The column is remembered across the rows.
    await page.keyboard.press(await primaryKey(page, 'End'));
    await expect(table(page).locator('[data-row="calculations"] [data-cell]').last()).toBeFocused();
    await page.keyboard.press(await primaryKey(page, 'Home'));
    await expect(header(page, 'Name')).toBeFocused();
    // Enter on a heading opens its menu; Escape closes it and the focus is back.
    await page.keyboard.press('Enter');
    await expect(menuItem(page, 'sortAscending')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menuItem(page, 'sortAscending')).toHaveCount(0);
    await expect(header(page, 'Name')).toBeFocused();
  });

  test('Space ticks a checkbox, and Enter on "+ New" makes a row', async ({ page }) => {
    await openView(page, 'Personal Tasks', 'List View');
    const id = await rowId(page, 'Buy groceries');
    const done = cellOf(page, id, await columnId(page, 'Completed'));
    const before = await done.getByRole('checkbox').isChecked();
    await done.focus();
    await page.keyboard.press('Space');
    await expect(done.getByRole('checkbox')).toBeChecked({ checked: !before });
    await expect(done).toBeFocused();

    const count = await cardRows(page).count();
    await table(page).locator('[data-kind="add"]').focus();
    await page.keyboard.press('Enter');
    await expect(cardRows(page)).toHaveCount(count + 1);
    await expect(cardRows(page).last().locator('input')).toBeFocused();
  });

  test('the buttons of a row are reached with Tab from its title', async ({ page }, testInfo) => {
    await openView(page, 'Project Tasks', 'Task Overview');
    const id = await rowId(page, 'Identify dependencies');
    await titleCell(page, id).focus();
    if (testInfo.project.name === 'desktop') {
      await page.keyboard.press(await tabKey(page));
      await expect(rowOf(page, id).locator('a[data-open-card]')).toBeFocused();
      // Focus shows what hover shows.
      await expect(rowOf(page, id).locator('a[data-open-card]').locator('xpath=..')).toHaveCSS(
        'opacity',
        '1',
      );
    }
    await page.keyboard.press(await tabKey(page));
    await expect(rowOf(page, id).locator('[data-more]')).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(menuItem(page, 'delete')).toBeVisible();
    await page.keyboard.press('Escape');
    // Escape on a button of the cell goes back to the cell.
    await expect(rowOf(page, id).locator('[data-more]')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(titleCell(page, id)).toBeFocused();
  });
});

// ------------------------------------------------------------ screenshots

test.describe('screenshots', () => {
  test.skip(!process.env.FVOCI_SCREENS, 'set FVOCI_SCREENS=1 to take the screenshots');

  for (const scheme of ['light', 'dark'] as const) {
    test(`the table, ${scheme}`, async ({ page }, testInfo) => {
      const stage = STAGES[testInfo.project.name] ?? testInfo.project.name;
      const shot = (name: string): Promise<Buffer> =>
        page.screenshot({ path: `${screensDir('table')}/${name}-${stage}-${scheme}.png` });
      await page.emulateMedia({ colorScheme: scheme });

      await openView(page, 'Project Tasks', 'Task Overview');
      await page.waitForLoadState('networkidle');
      await page.evaluate(() => document.fonts.ready);
      await shot('table');

      // Scrolled sideways: the title column stays and casts its shadow.
      await table(page).evaluate((element) => {
        element.scrollLeft = 120;
      });
      await expect(table(page)).toHaveAttribute('data-scrolled', '');
      await page.waitForTimeout(250);
      await shot('table-scrolled');

      // A cell being edited: the options of a select.
      await table(page).evaluate((element) => {
        element.scrollLeft = 0;
      });
      const id = await rowId(page, 'Identify dependencies');
      await press(cellOf(page, id, await columnId(page, 'Priority')));
      await expect(page.locator('[data-presentation]')).toBeVisible();
      await page.waitForTimeout(350);
      await shot('table-select');
      await page.keyboard.press('Escape');

      // The menu of a heading.
      await openHeaderMenu(page, 'Status');
      await page.waitForTimeout(350);
      await shot('table-column-menu');
      await page.keyboard.press('Escape');

      await openView(page, 'Sprint Planner', 'By Sprint');
      await page.waitForLoadState('networkidle');
      await shot('table-grouped');

      await openView(page, 'Company Goals & OKRs', 'By Quarter');
      await page.waitForLoadState('networkidle');
      await shot('table-read-only');
    });
  }
});

// ------------------------------------------------------------ a big table

test.describe('five hundred rows', () => {
  test.skip(!process.env.FVOCI_MEASURE, 'set FVOCI_MEASURE=1 to measure');
  // Run it with `--trace off --workers 1`: recording a trace takes a snapshot of the page at
  // every step, and that would be what is measured.

  test('how long the first drawing takes, and how scrolling goes', async ({ page }, testInfo) => {
    test.setTimeout(120_000);
    await page.goto('/');
    await expect(page.getByTestId('app-shell')).toBeVisible();
    // The cards are made while another board is shown; then the table is opened on all of them at once.
    const target = await page.evaluate(async () => {
      const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
      const [workspace] = await backend.workspaces();
      const boards = await backend.boards(workspace?.id ?? '');
      const board = boards.find((each) => each.title === 'Sales Pipeline CRM');
      const other = boards.find((each) => each.title === 'Personal Tasks');
      if (!board || !other) throw new Error('the seed has no such boards');
      const view = (await backend.views(board.id)).find((each) => each.title === 'All Contacts');
      const start = (await backend.views(other.id)).find((each) => each.title === 'List View');
      if (!view || !start) throw new Error('the seed has no such views');
      backend.latency = 0;
      for (let at = 0; at < 495; at += 1) {
        await backend.createCard(board.id, { title: `Contact ${String(at + 1).padStart(3, '0')}` });
      }
      return { board: board.id, view: view.id, other: other.id, start: start.id };
    });
    const go = (path: string): Promise<void> =>
      page.evaluate((to) => {
        history.pushState(null, '', to);
        window.dispatchEvent(new PopStateEvent('popstate'));
      }, path);
    await go(boardPath(target.other, target.start));
    await expect(cardRows(page)).toHaveCount(5);

    const firstDraw = await page.evaluate(
      async (to) => {
        const started = performance.now();
        history.pushState(null, '', to);
        window.dispatchEvent(new PopStateEvent('popstate'));
        const rows = (): number =>
          document.querySelectorAll('[data-testid="table"] [data-kind="title"]').length;
        while (rows() < 500) await new Promise((resolve) => setTimeout(resolve, 0));
        const drawn = performance.now();
        // Two frames: the rows are laid out and painted.
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        return { toRows: drawn - started, toPaint: performance.now() - started };
      },
      boardPath(target.board, target.view),
    );
    await expect(cardRows(page)).toHaveCount(500);

    const nodes = await table(page).evaluate((element) => element.querySelectorAll('*').length);
    const scroll = await table(page).evaluate(async (element) => {
      const frames: number[] = [];
      let last = performance.now();
      for (let at = 0; at < 120; at += 1) {
        element.scrollTop += 60;
        element.scrollLeft = (at % 40) * 10;
        await new Promise((resolve) => requestAnimationFrame(resolve));
        const now = performance.now();
        frames.push(now - last);
        last = now;
      }
      frames.sort((a, b) => a - b);
      return {
        median: frames[Math.floor(frames.length / 2)],
        p95: frames[Math.floor(frames.length * 0.95)],
        worst: frames.at(-1),
      };
    });
    // One cell edited among five hundred rows.
    const edit = await page.evaluate(async () => {
      const cell = document.querySelector<HTMLElement>(
        '[data-testid="table"] [data-nav-row]:nth-of-type(250) [data-kind="value"]',
      );
      const started = performance.now();
      cell?.click();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return performance.now() - started;
    });
    const round = (value: number | undefined): number => Math.round((value ?? 0) * 10) / 10;
    // A figure for the report, not a check: printed where the test runner shows it.
    console.warn(
      `[${testInfo.project.name}] 500 rows × 13 columns: ${nodes} elements; ` +
        `first drawing ${round(firstDraw.toRows)} ms to the rows, ${round(firstDraw.toPaint)} ms to the second frame after; ` +
        `scrolling 120 frames: median ${round(scroll.median)} ms, 95th ${round(scroll.p95)} ms, worst ${round(scroll.worst)} ms; ` +
        `pressing a cell: ${round(edit)} ms to the second frame after`,
    );
  });
});
