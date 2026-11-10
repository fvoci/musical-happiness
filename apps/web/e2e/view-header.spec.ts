/**
 * The header of a view, on the app with its in-memory backend: the whole row
 * and the folded one, the views and what is done to them, the properties a
 * view shows, grouping, the filter, the sort, and search.
 *
 * The three projects are the three stages of a page. Unless a test says
 * otherwise it runs in all of them, and reaches each tool the way that
 * layout offers it: by its own button in the whole row, through "…" in the
 * folded one.
 *
 * The seed is the same on every load. The first board of the sidebar is
 * "Project Tasks": five cards, the views "Progress Tracker" (a board, the
 * first tab), "Project Priorities" (a board), "Task Calendar" and "Task
 * Overview" (a table without grouping, where the cards are a plain list in
 * the order of the view; the tests that count and order cards use it).
 */
import { mkdirSync } from 'node:fs';
import path from 'node:path';

import { expect, test, type Locator, type Page } from '@playwright/test';

interface StoredView {
  id: string;
  title: string;
  fields: {
    viewType: string;
    groupById?: string;
    visiblePropertyIds: string[];
    visibleOptionIds: string[];
    hiddenOptionIds: string[];
    sortOptions: { propertyId: string; reversed: boolean }[];
    filter: { operation: string; filters: unknown[] };
    columnWidths: Record<string, number>;
  };
}

interface Dev {
  backend: {
    teams: () => Promise<{ id: string }[]>;
    boards: (teamId: string) => Promise<{ id: string; title: string }[]>;
    board: (boardId: string) => Promise<{
      cardProperties: { id: string; name: string; options: { id: string; value: string }[] }[];
    }>;
    views: (boardId: string) => Promise<StoredView[]>;
    patchView: (boardId: string, viewId: string, patch: object) => Promise<unknown>;
    /** How long it takes to answer, in milliseconds. */
    latency: number;
  };
  workspace: { undoLast: () => Promise<void> };
}

const header = (page: Page): Locator => page.getByTestId('view-header');
const tool = (page: Page, name: string): Locator => header(page).locator(`[data-tool="${name}"]`);
const cards = (page: Page): Locator => page.locator('[data-card]');
const layout = (page: Page): Promise<string | null> => header(page).getAttribute('data-layout');

/**
 * The shell is a stand-in for now and keeps its list of boards beside the
 * page at every width, which on a compact page leaves the board a sliver no
 * device will have. There the list is taken out, so that the pane is the
 * page, as it is once the list is a drawer. A shell whose list takes no room
 * on a compact page is left alone.
 */
async function givePaneItsRoom(page: Page): Promise<void> {
  await page.evaluate(() => {
    const shell = document.querySelector('[data-testid="app-shell"]');
    const main = shell?.querySelector('main');
    const list = shell?.querySelector<HTMLElement>(':scope > nav');
    if (!main || !list) return;
    if (list.dataset.takenOut !== undefined) {
      list.style.display = '';
      delete list.dataset.takenOut;
    }
    if (window.innerWidth < 600 && main.clientWidth < window.innerWidth - 100) {
      list.style.display = 'none';
      list.dataset.takenOut = '';
    }
  });
}

async function open(page: Page, address = '/'): Promise<void> {
  await page.goto(address);
  await expect(header(page)).toBeVisible();
  await givePaneItsRoom(page);
}

async function resize(page: Page, width: number, height: number): Promise<void> {
  await page.setViewportSize({ width, height });
  await givePaneItsRoom(page);
}

/** Makes the window as wide as gives the pane this width, whatever the sidebar takes. */
async function setPaneWidth(page: Page, width: number): Promise<void> {
  const size = page.viewportSize();
  if (!size) throw new Error('no viewport');
  const pane = await header(page).evaluate((element) => element.getBoundingClientRect().width);
  await page.setViewportSize({ width: Math.round(size.width + width - pane), height: size.height });
  await expect
    .poll(() => header(page).evaluate((element) => element.getBoundingClientRect().width))
    .toBe(width);
}

/** The address of a board of the seed, found by its title. */
async function boardAddress(page: Page, title: string): Promise<string> {
  const id = await page.evaluate(async (wanted) => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const [team] = await backend.teams();
    if (!team) throw new Error('no team');
    return (await backend.boards(team.id)).find((board) => board.title === wanted)?.id;
  }, title);
  if (!id) throw new Error(`no board called ${title}`);
  return `/board/${id}`;
}

/** The view the address names, as the backend has it. */
function storedView(page: Page): Promise<StoredView> {
  return page.evaluate(async () => {
    const [, , boardId, viewId] = location.pathname.split('/');
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const view = (await backend.views(boardId ?? '')).find((each) => each.id === viewId);
    if (!view) throw new Error('the address names no view');
    return view;
  });
}

/** The properties of the board the address names, by name. */
function properties(
  page: Page,
): Promise<Record<string, { id: string; options: Record<string, string> }>> {
  return page.evaluate(async () => {
    const [, , boardId] = location.pathname.split('/');
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const board = await backend.board(boardId ?? '');
    return Object.fromEntries(
      board.cardProperties.map((property) => [
        property.name,
        {
          id: property.id,
          options: Object.fromEntries(property.options.map((option) => [option.value, option.id])),
        },
      ]),
    );
  });
}

/** Changes the view the address names as the backend, without the screen: for setting a scene. */
async function patchView(page: Page, updatedFields: object): Promise<void> {
  await page.evaluate(async (fields) => {
    const [, , boardId, viewId] = location.pathname.split('/');
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    await backend.patchView(boardId ?? '', viewId ?? '', { updatedFields: fields });
  }, updatedFields);
}

/**
 * Undoes the last change. A change can be undone once the backend has
 * answered it, and the in-memory one takes its time to answer; a timer of the
 * page set now, for longer than that, rings after the answers of everything
 * that was sent before.
 */
async function undo(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const { backend, workspace } = (window as unknown as { fvociDev: Dev }).fvociDev;
    await new Promise((resolve) => setTimeout(resolve, backend.latency + 20));
    await workspace.undoLast();
  });
}

/** Shows another view of the board: by its tab, or through the list behind the button. */
async function showView(page: Page, name: string): Promise<void> {
  if ((await layout(page)) === 'row') {
    await header(page).getByRole('link', { name, exact: true }).click();
    await expect(header(page).getByRole('link', { name, exact: true })).toHaveAttribute(
      'aria-current',
      'page',
    );
  } else {
    await tool(page, 'views').click();
    await page.getByRole('menuitem', { name, exact: true }).click();
    await expect(tool(page, 'views')).toContainText(name);
  }
}

/** Opens a tool: by its own button where the row has one, through "…" otherwise. */
async function openTool(
  page: Page,
  name: 'properties' | 'group' | 'filter' | 'sort',
): Promise<void> {
  if (name === 'filter' || (await layout(page)) === 'row') {
    await tool(page, name).click();
  } else {
    await tool(page, 'more').click();
    await page.locator(`[role="menuitem"][data-action="${name}"]`).click();
  }
}

/** A line of "…". */
async function viewCommand(page: Page, name: 'rename' | 'duplicate' | 'delete'): Promise<void> {
  await tool(page, 'more').click();
  await page.locator(`[role="menuitem"][data-action="${name}"]`).click();
}

/** Closes what is open on top: a popover, a menu or a sheet. */
async function closeTop(page: Page): Promise<void> {
  await page.keyboard.press('Escape');
}

const sheet = (page: Page): Locator => page.getByTestId('bottom-sheet');
const filterEditor = (page: Page): Locator => page.getByTestId('filter-editor');
const sortEditor = (page: Page): Locator => page.getByTestId('sort-editor');
const filterRows = (page: Page): Locator => filterEditor(page).locator('[data-filter-row]');

/** Adds a condition to the open filter editor and puts it on a property. */
async function addCondition(page: Page, property: string, condition?: string): Promise<Locator> {
  const before = await filterRows(page).count();
  await filterEditor(page).locator('[data-tool="add-filter"]').click();
  await expect(filterRows(page)).toHaveCount(before + 1);
  const row = filterRows(page).nth(before);
  await row.locator('[data-slot="filter-property"]').click();
  await page.getByRole('menuitem', { name: property, exact: true }).click();
  await expect(row.locator('[data-slot="filter-property"]')).toContainText(property);
  if (condition !== undefined) {
    await row.locator('[data-slot="filter-condition"]').click();
    await page.getByRole('menuitem', { name: condition, exact: true }).click();
    await expect(row.locator('[data-slot="filter-condition"]')).toContainText(condition);
  }
  return row;
}

async function addSort(page: Page, property: string): Promise<void> {
  await sortEditor(page).locator('[data-tool="add-sort"]').click();
  await page.getByRole('menuitem', { name: property, exact: true }).click();
  await expect(sortEditor(page).locator('[data-sort]').filter({ hasText: property })).toBeVisible();
}

/** The titles of the cards the view shows, top to bottom. */
async function titles(page: Page): Promise<string[]> {
  return (await cards(page).allTextContents()).map((text) => text.trim());
}

/** A card is found by its title in whatever else its line shows. */
const containing = (text: string): unknown => expect.stringContaining(text);

const ALL = [
  'Identify dependencies',
  'Define project scope',
  'Requirements sign-off',
  'Project budget approval',
  'Conduct market analysis',
];

test.describe('the two rows', () => {
  test('a pane of 992 pixels has the whole header in one row, and nothing spills', async ({
    page,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'needs a window that can be this wide');
    await open(page);
    // 1280 wide, the sidebar beside it: the pane is 1024.
    await expect(header(page)).toHaveAttribute('data-layout', 'row');

    await setPaneWidth(page, 992);
    await expect(header(page)).toHaveAttribute('data-layout', 'row');
    for (const name of ['properties', 'group', 'filter', 'sort', 'more', 'add-view']) {
      await expect(tool(page, name)).toBeVisible();
    }
    await expect(header(page).getByRole('searchbox', { name: 'Search cards' })).toBeVisible();
    await expect(header(page).getByRole('link', { name: 'Progress Tracker' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    // "New" is in the row, and the row is as wide as its content.
    await expect(header(page).getByRole('button', { name: 'New', exact: true })).toBeVisible();
    const spill = await header(page).evaluate((element) => {
      const edge = element.getBoundingClientRect().right;
      const beyond = [...element.querySelectorAll('a, button, input')].filter(
        (each) => each.getBoundingClientRect().right > edge + 0.5,
      );
      return { scroll: element.scrollWidth - element.clientWidth, beyond: beyond.length };
    });
    expect(spill).toEqual({ scroll: 0, beyond: 0 });
    expect(await header(page).evaluate((element) => element.getBoundingClientRect().height)).toBe(
      48,
    );

    // One pixel less and it folds.
    await setPaneWidth(page, 991);
    await expect(header(page)).toHaveAttribute('data-layout', 'folded');
  });

  test('a narrower pane folds the header, and "New" becomes a round button at the bottom', async ({
    page,
  }, testInfo) => {
    await open(page);
    // A window of 1100 with the sidebar beside the board: a pane of 844.
    if (testInfo.project.name === 'desktop') await resize(page, 1100, 720);
    await expect(header(page)).toHaveAttribute('data-layout', 'folded');

    await expect(tool(page, 'views')).toContainText('Progress Tracker');
    for (const name of ['search', 'filter', 'more']) await expect(tool(page, name)).toBeVisible();
    for (const name of ['properties', 'group', 'sort']) await expect(tool(page, name)).toBeHidden();
    await expect(header(page).getByRole('link')).toHaveCount(0);

    const add = page.getByTestId('new-card');
    await expect(add).toBeVisible();
    await expect(add).toHaveAccessibleName('New');
    const place = await add.evaluate((element) => {
      const box = element.getBoundingClientRect();
      const board = element.closest('[data-testid="board-page"]')?.getBoundingClientRect();
      return {
        width: box.width,
        height: box.height,
        round: getComputedStyle(element).borderRadius,
        fromRight: (board?.right ?? 0) - box.right,
        fromBottom: (board?.bottom ?? 0) - box.bottom,
        inHeader: element.closest('[data-testid="view-header"]') !== null,
      };
    });
    expect(place).toMatchObject({ width: 56, height: 56, fromRight: 16, fromBottom: 16 });
    expect(place.inHeader).toBe(false);
    expect(parseFloat(place.round)).toBeGreaterThanOrEqual(28);

    // It makes a card and opens it, like "New" in the row.
    await add.click();
    await expect(page.getByTestId('card-dialog')).toBeVisible();
  });

  test('a pane that gets wider unfolds the header, and folds it again when it narrows', async ({
    page,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'needs a window that can be this wide');
    await open(page);
    await resize(page, 1100, 720);
    await expect(header(page)).toHaveAttribute('data-layout', 'folded');
    // The sidebar cannot be folded away yet; a wider window gives the pane the same room.
    await resize(page, 1300, 720);
    await expect(header(page)).toHaveAttribute('data-layout', 'row');
    await expect(page.getByTestId('new-card')).toHaveText(/New/);
    await resize(page, 1100, 720);
    await expect(header(page)).toHaveAttribute('data-layout', 'folded');
  });
});

test.describe('in Korean', () => {
  test.use({ locale: 'ko-KR' });

  test('the whole row has its words, and still does not spill at 992 pixels', async ({
    page,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'needs a window that can be this wide');
    await open(page);
    await setPaneWidth(page, 992);
    await expect(header(page)).toHaveAttribute('data-layout', 'row');
    await expect(tool(page, 'properties')).toHaveText('속성');
    await expect(tool(page, 'group')).toHaveText(/그룹화 기준: Status/);
    await expect(tool(page, 'filter')).toHaveText('필터');
    await expect(tool(page, 'sort')).toHaveText('정렬');
    await expect(header(page).getByRole('searchbox', { name: '카드 검색' })).toBeVisible();
    await expect(page.getByTestId('new-card')).toHaveText(/새로 만들기/);
    const spill = await header(page).evaluate((element) => {
      const edge = element.getBoundingClientRect().right;
      const beyond = [...element.querySelectorAll('a, button, input')].filter(
        (each) => each.getBoundingClientRect().right > edge + 0.5,
      );
      return { scroll: element.scrollWidth - element.clientWidth, beyond: beyond.length };
    });
    expect(spill).toEqual({ scroll: 0, beyond: 0 });

    // The words of the editors are Korean too.
    await tool(page, 'filter').click();
    await expect(filterEditor(page)).toContainText('조건이 없습니다');
    await filterEditor(page).locator('[data-tool="add-filter"]').click();
    await expect(filterRows(page).first().locator('[data-slot="filter-condition"]')).toContainText(
      '포함함',
    );
  });
});

test.describe('views', () => {
  test('choosing a view shows it and changes the address', async ({ page }) => {
    await open(page);
    const first = page.url();
    await showView(page, 'Task Overview');
    await expect(page).not.toHaveURL(first);
    const overview = page.url();
    expect((await storedView(page)).title).toBe('Task Overview');

    // The address is the view: loaded on its own it shows the same one.
    await open(page, overview);
    expect((await storedView(page)).title).toBe('Task Overview');
    if ((await layout(page)) === 'row') {
      await expect(header(page).getByRole('link', { name: 'Task Overview' })).toHaveAttribute(
        'aria-current',
        'page',
      );
      // A tab is a link to its view.
      const href = await header(page)
        .getByRole('link', { name: 'Progress Tracker' })
        .getAttribute('href');
      expect(new URL(href ?? '', page.url()).href).toBe(first);
    } else {
      await expect(tool(page, 'views')).toContainText('Task Overview');
    }

    await page.goBack();
    await expect(page).toHaveURL(first);
  });

  test('a view is added, renamed, copied and deleted', async ({ page }) => {
    await open(page);
    const start = page.url();
    const everyProperty = Object.values(await properties(page)).map((property) => property.id);

    // Added: a table that shows every property.
    if ((await layout(page)) === 'row') {
      await tool(page, 'add-view').click();
      await page.getByRole('menuitem', { name: 'Table', exact: true }).click();
    } else {
      await tool(page, 'views').click();
      await page.getByRole('menuitem', { name: 'Add table view' }).click();
    }
    await expect(page).not.toHaveURL(start);
    await expect.poll(async () => (await storedView(page)).title).toBe('Table view');
    const added = await storedView(page);
    expect(added.fields.viewType).toBe('table');
    expect(added.fields.visiblePropertyIds).toEqual(everyProperty);
    expect(added.fields.columnWidths).toEqual({ __title: 280 });

    // Renamed.
    await viewCommand(page, 'rename');
    const name = page.getByRole('textbox', { name: 'View name' });
    await expect(name).toHaveValue('Table view');
    await name.fill('Mine');
    await name.press('Enter');
    await expect(name).toBeHidden();
    await expect.poll(async () => (await storedView(page)).title).toBe('Mine');
    await expect(header(page)).toContainText('Mine');

    // Escape leaves the name as it was.
    await viewCommand(page, 'rename');
    await name.fill('Not this');
    await name.press('Escape');
    await expect(name).toBeHidden();
    expect((await storedView(page)).title).toBe('Mine');

    // Copied: the copy is shown.
    const original = page.url();
    await viewCommand(page, 'duplicate');
    await expect(page).not.toHaveURL(original);
    await expect.poll(async () => (await storedView(page)).title).toBe('Mine copy');
    expect((await storedView(page)).fields.visiblePropertyIds).toEqual(everyProperty);

    // Deleted, after asking: another view of the board is shown.
    const copy = page.url();
    await viewCommand(page, 'delete');
    const confirm = page.getByTestId('confirm-dialog');
    await expect(confirm).toContainText('Delete “Mine copy”?');
    await confirm.getByRole('button', { name: 'Cancel' }).click();
    await expect(confirm).toBeHidden();
    expect(page.url()).toBe(copy);

    await viewCommand(page, 'delete');
    await confirm.getByRole('button', { name: 'Delete' }).click();
    await expect(page).not.toHaveURL(copy);
    await expect(header(page)).not.toContainText('Mine copy');
    await expect(header(page)).toBeVisible();

    // Undo brings the view back.
    await undo(page);
    await showView(page, 'Mine copy');
    expect(page.url()).toBe(copy);
  });

  test('the last view of a board cannot be deleted', async ({ page }) => {
    await open(page);
    await open(page, await boardAddress(page, 'Meeting Agenda'));
    await tool(page, 'more').click();
    await expect(page.locator('[role="menuitem"][data-action="delete"]')).toBeDisabled();
    await expect(page.locator('[role="menuitem"][data-action="duplicate"]')).toBeEnabled();
  });
});

test.describe('what a view shows', () => {
  test('properties are switched on and off, each as it is pressed', async ({ page }) => {
    await open(page);
    const assignee = (await properties(page)).Assignee?.id ?? '';
    expect((await storedView(page)).fields.visiblePropertyIds).not.toContain(assignee);

    await openTool(page, 'properties');
    const panel = page.getByTestId('properties-panel');
    const toggle = panel.getByRole('switch', { name: 'Assignee' });
    await expect(toggle).toHaveAttribute('aria-checked', 'false');
    const priority = panel.getByRole('switch', { name: 'Priority' });
    await expect(priority).toHaveAttribute('aria-checked', 'true');
    // A board can show the marks of comments and description; it has no switch for the title.
    await expect(panel.getByRole('switch', { name: 'Comments and description' })).toBeVisible();
    await expect(panel.getByRole('switch', { name: 'Title' })).toHaveCount(0);

    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-checked', 'true');
    await expect
      .poll(async () => (await storedView(page)).fields.visiblePropertyIds)
      .toContain(assignee);
    // The list stays open for the next one.
    await priority.click();
    await expect(priority).toHaveAttribute('aria-checked', 'false');

    // Each switch was one change: one undo brings Priority back, the next takes Assignee out.
    await undo(page);
    await expect(priority).toHaveAttribute('aria-checked', 'true');
    await expect(toggle).toHaveAttribute('aria-checked', 'true');
    await undo(page);
    await expect(toggle).toHaveAttribute('aria-checked', 'false');
    expect((await storedView(page)).fields.visiblePropertyIds).not.toContain(assignee);
  });

  test('grouping by another property starts its columns over, and one undo takes it back', async ({
    page,
  }) => {
    await open(page);
    const all = await properties(page);
    const before = await storedView(page);
    expect(before.fields.groupById).toBe(all.Status?.id);
    // Hide a column, so that there is something for the change to clear.
    const firstStatus = Object.values(all.Status?.options ?? {})[0] ?? '';
    await patchView(page, {
      visibleOptionIds: before.fields.visibleOptionIds.filter((id) => id !== firstStatus),
      hiddenOptionIds: [firstStatus],
    });
    const hidden = await storedView(page);

    await openTool(page, 'group');
    const choices = page.getByRole('menuitem');
    // A board is always grouped: there is no "none" to pick.
    await expect(choices.filter({ hasText: 'Ungroup' })).toHaveCount(0);
    await expect(choices.filter({ hasText: 'Estimated Hours' })).toHaveCount(0);
    await choices.filter({ hasText: 'Priority' }).click();
    // Picking one closes the list.
    await expect(choices).toHaveCount(0);

    await expect.poll(async () => (await storedView(page)).fields.groupById).toBe(all.Priority?.id);
    const after = await storedView(page);
    expect(after.fields.visibleOptionIds).toEqual([
      '',
      ...Object.values(all.Priority?.options ?? {}),
    ]);
    expect(after.fields.hiddenOptionIds).toEqual([]);
    if ((await layout(page)) === 'row') {
      await expect(tool(page, 'group')).toHaveText(/Group by: Priority/);
    }

    await undo(page);
    await expect.poll(async () => (await storedView(page)).fields.groupById).toBe(all.Status?.id);
    const undone = await storedView(page);
    expect(undone.fields.visibleOptionIds).toEqual(hidden.fields.visibleOptionIds);
    expect(undone.fields.hiddenOptionIds).toEqual([firstStatus]);
  });

  test('a table can be grouped by nothing', async ({ page }) => {
    await open(page);
    await showView(page, 'Task Overview');
    const all = await properties(page);
    expect((await storedView(page)).fields.groupById).toBe('');

    await openTool(page, 'group');
    await page.getByRole('menuitem', { name: 'Status', exact: true }).click();
    await expect.poll(async () => (await storedView(page)).fields.groupById).toBe(all.Status?.id);

    await openTool(page, 'group');
    await page.getByRole('menuitem', { name: 'Ungroup', exact: true }).click();
    await expect.poll(async () => (await storedView(page)).fields.groupById).toBe('');
    expect((await storedView(page)).fields.visibleOptionIds).toEqual([]);
  });
});

test.describe('filter', () => {
  test.beforeEach(async ({ page }) => {
    await open(page);
    await showView(page, 'Task Overview');
    await expect(cards(page)).toHaveCount(5);
  });

  test('options, text, a date and emptiness each change how many cards there are', async ({
    page,
  }) => {
    await openTool(page, 'filter');
    await expect(filterEditor(page)).toContainText('No conditions');

    // Options: ticked in a list that stays open.
    const priority = await addCondition(page, 'Priority');
    await expect(priority.locator('[data-slot="filter-condition"]')).toContainText('includes');
    await expect(cards(page)).toHaveCount(5);
    await priority.locator('[data-slot="property-select"]').click();
    await page.getByRole('option', { name: '1. High' }).click();
    await expect(cards(page)).toHaveCount(2);
    await page.getByRole('option', { name: '3. Low' }).click();
    await expect(cards(page)).toHaveCount(3);
    await closeTop(page);
    await expect(filterEditor(page)).toBeVisible();
    await expect(tool(page, 'filter').locator('[data-count]')).toHaveText('1');

    // The same options, the other way round.
    await priority.locator('[data-slot="filter-condition"]').click();
    await page.getByRole('menuitem', { name: "doesn't include", exact: true }).click();
    await expect(cards(page)).toHaveCount(2);

    // Emptiness has nothing to compare with.
    await priority.locator('[data-slot="filter-condition"]').click();
    await page.getByRole('menuitem', { name: 'is empty', exact: true }).click();
    await expect(cards(page)).toHaveCount(0);
    await expect(priority.locator('[data-slot="property-select"]')).toHaveCount(0);
    await priority.locator('[data-slot="filter-condition"]').click();
    await page.getByRole('menuitem', { name: 'is not empty', exact: true }).click();
    await expect(cards(page)).toHaveCount(5);

    // Another property: the condition becomes one its kind of value has.
    await priority.locator('[data-slot="filter-property"]').click();
    await page.getByRole('menuitem', { name: 'Title', exact: true }).click();
    const title = filterRows(page).first();
    await expect(title.locator('[data-slot="filter-condition"]')).toContainText('is');
    await title.locator('[data-slot="filter-condition"]').click();
    await page.getByRole('menuitem', { name: 'contains', exact: true }).click();
    const text = title.getByRole('textbox', { name: 'Value for Title' });
    await text.fill('PROJECT');
    await text.press('Enter');
    await expect(cards(page)).toHaveCount(2);
    expect((await titles(page)).sort()).toEqual([
      expect.stringContaining('Define project scope'),
      expect.stringContaining('Project budget approval'),
    ]);

    // A date: a day picked from the calendar. The cards were all made before today.
    await title.locator('[data-remove]').click();
    await expect(filterRows(page)).toHaveCount(0);
    await expect(cards(page)).toHaveCount(5);
    const created = await addCondition(page, 'Date Created', 'is after');
    await created.locator('[data-slot="filter-date"]').click();
    await page.getByRole('button', { name: 'Today', exact: true }).click();
    await expect(created.locator('[data-slot="filter-date"]')).not.toContainText('Empty');
    await expect(cards(page)).toHaveCount(0);
    await created.locator('[data-slot="filter-condition"]').click();
    await page.getByRole('menuitem', { name: 'is before', exact: true }).click();
    await expect(cards(page)).toHaveCount(5);
    const stored = (await storedView(page)).fields.filter.filters[0] as { values: string[] };
    expect(stored.values[0]).toMatch(/^\d{4}-\d{2}-\d{2}$/);

    // No card has a due date.
    await created.locator('[data-slot="filter-property"]').click();
    await page.getByRole('menuitem', { name: 'Due Date', exact: true }).click();
    await created.locator('[data-slot="filter-condition"]').click();
    await page.getByRole('menuitem', { name: 'is set', exact: true }).click();
    await expect(cards(page)).toHaveCount(0);
    await expect(created.locator('[data-slot="filter-date"]')).toHaveCount(0);
  });

  test('several conditions: all of them, or any one; and undo, a step at a time', async ({
    page,
  }) => {
    await openTool(page, 'filter');
    const priority = await addCondition(page, 'Priority');
    await priority.locator('[data-slot="property-select"]').click();
    await page.getByRole('option', { name: '1. High' }).click();
    await closeTop(page);
    await expect(cards(page)).toHaveCount(2);
    // With one condition there is nothing to choose between.
    await expect(filterEditor(page).getByRole('radiogroup')).toHaveCount(0);

    const title = await addCondition(page, 'Title', 'contains');
    const text = title.getByRole('textbox', { name: 'Value for Title' });
    await text.fill('project');
    await text.press('Enter');
    await expect(cards(page)).toHaveCount(1);
    await expect(cards(page)).toContainText('Project budget approval');
    await expect(tool(page, 'filter').locator('[data-count]')).toHaveText('2');

    const match = filterEditor(page).getByRole('radiogroup');
    await expect(match.getByRole('radio', { name: 'All conditions' })).toBeChecked();
    await match.getByRole('radio', { name: 'Any condition' }).click();
    await expect(cards(page)).toHaveCount(3);
    expect((await storedView(page)).fields.filter.operation).toBe('or');

    // Undo: back to "all", then the text goes, then the condition on the title.
    await undo(page);
    await expect(cards(page)).toHaveCount(1);
    await expect(match.getByRole('radio', { name: 'All conditions' })).toBeChecked();
    await undo(page);
    await expect(cards(page)).toHaveCount(2);
    await expect(text).toHaveValue('');

    // A person: the members of the board, ticked like options.
    await title.locator('[data-slot="filter-property"]').click();
    await page.getByRole('menuitem', { name: 'Assignee', exact: true }).click();
    await title.locator('[data-slot="property-person"]').click();
    const people = page.getByRole('option');
    await expect(people.first()).toBeVisible();
    expect(await people.count()).toBeGreaterThanOrEqual(3);
    await people.first().click();
    await closeTop(page);
    const assignee = (await storedView(page)).fields.filter.filters[1] as { values: string[] };
    expect(assignee.values).toHaveLength(1);
  });

  test('a group of conditions inside the filter is said to be there, and survives every edit', async ({
    page,
  }) => {
    const all = await properties(page);
    const nested = {
      operation: 'or',
      filters: [
        { propertyId: 'title', condition: 'contains', values: ['scope'] },
        { propertyId: 'title', condition: 'contains', values: ['budget'] },
      ],
    };
    await patchView(page, { filter: { operation: 'and', filters: [nested] } });
    await expect(cards(page)).toHaveCount(2);
    await expect(tool(page, 'filter').locator('[data-count]')).toHaveText('2');

    await openTool(page, 'filter');
    await expect(page.getByTestId('filter-nested')).toContainText('Nested groups of conditions: 1');
    await expect(filterRows(page)).toHaveCount(0);

    // Add a condition, change it, change how they combine, remove it: the group is as it was.
    const priority = await addCondition(page, 'Priority');
    await priority.locator('[data-slot="property-select"]').click();
    await page.getByRole('option', { name: '2. Medium' }).click();
    await closeTop(page);
    await expect(cards(page)).toHaveCount(1);
    expect((await storedView(page)).fields.filter).toEqual({
      operation: 'and',
      filters: [
        nested,
        {
          propertyId: all.Priority?.id,
          condition: 'includes',
          values: [all.Priority?.options['2. Medium']],
        },
      ],
    });
    await filterEditor(page).getByRole('radio', { name: 'Any condition' }).click();
    await expect(cards(page)).toHaveCount(3);
    await priority.locator('[data-remove]').click();
    await expect(filterRows(page)).toHaveCount(0);
    expect((await storedView(page)).fields.filter).toEqual({ operation: 'or', filters: [nested] });
    await expect(cards(page)).toHaveCount(2);
  });
});

test.describe('sort', () => {
  test('several sorts apply in their order, each in its direction', async ({ page }) => {
    await open(page);
    await showView(page, 'Task Overview');
    await expect(cards(page)).toHaveCount(5);
    const all = await properties(page);

    await openTool(page, 'sort');
    await expect(sortEditor(page)).toContainText('No sort');
    await expect(page.getByTestId('sort-notice')).toHaveCount(0);

    await addSort(page, 'Title');
    await expect.poll(() => titles(page)).toEqual([...ALL].sort().map(containing));
    // A table is one of the views whose cards are dragged: the editor says what the sort turns off.
    await expect(page.getByTestId('sort-notice')).toContainText('cannot be dragged');
    await expect(tool(page, 'sort').locator('[data-count]')).toHaveText('1');

    // Reversed.
    const byTitle = sortEditor(page).locator('[data-sort="__title"]');
    await byTitle.getByRole('button', { name: /Title: Ascending/ }).click();
    await expect(byTitle.getByRole('button', { name: /Title: Descending/ })).toBeVisible();
    await expect.poll(() => titles(page)).toEqual([...ALL].sort().reverse().map(containing));

    // A second sort only orders what the first leaves equal: here nothing.
    await addSort(page, 'Priority');
    await expect.poll(() => titles(page)).toEqual([...ALL].sort().reverse().map(containing));
    expect((await storedView(page)).fields.sortOptions).toEqual([
      { propertyId: '__title', reversed: true },
      { propertyId: all.Priority?.id, reversed: false },
    ]);

    // Moved up, the priority decides first and the title, still reversed, within it.
    const byPriority = sortEditor(page).locator(`[data-sort="${all.Priority?.id ?? ''}"]`);
    await expect(byPriority.getByRole('button', { name: 'Move Priority down' })).toBeDisabled();
    await byPriority.getByRole('button', { name: 'Move Priority up' }).click();
    await expect(sortEditor(page).getByRole('status')).toHaveText('Priority is now number 1 of 2.');
    await expect(byPriority.getByRole('button', { name: 'Move Priority up' })).toBeDisabled();
    // The focus went with the line, to the button that still works.
    await expect(byPriority.getByRole('button', { name: 'Move Priority down' })).toBeFocused();
    await expect
      .poll(() => titles(page))
      .toEqual(
        [
          'Requirements sign-off',
          'Project budget approval',
          'Define project scope',
          'Conduct market analysis',
          'Identify dependencies',
        ].map(containing),
      );

    // A property is sorted by once: it is no longer offered.
    await sortEditor(page).locator('[data-tool="add-sort"]').click();
    await expect(page.getByRole('menuitem', { name: 'Status', exact: true })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: 'Priority', exact: true })).toHaveCount(0);
    await closeTop(page);

    // Removed: the title alone again. The focus is not left on a button that is gone.
    await byPriority.getByRole('button', { name: 'Remove the sort by Priority' }).click();
    await expect(byPriority).toHaveCount(0);
    await expect(sortEditor(page).locator('[data-tool="add-sort"]')).toBeFocused();
    await expect.poll(() => titles(page)).toEqual([...ALL].sort().reverse().map(containing));

    await undo(page);
    await expect(sortEditor(page).locator('[data-sort]')).toHaveCount(2);
  });
});

test.describe('search', () => {
  test('narrows the cards as it is typed; closing it brings them back', async ({ page }) => {
    await open(page);
    await showView(page, 'Task Overview');
    await expect(cards(page)).toHaveCount(5);
    const folded = (await layout(page)) === 'folded';
    // By its element: while the folded row has it away, it has no role to be found by.
    const field = header(page).locator('input[type="search"]');

    if (folded) {
      await expect(field).toBeHidden();
      await tool(page, 'search').click();
      // The row is the search field now, and the field has the focus.
      await expect(field).toBeFocused();
      await expect(header(page).getByRole('searchbox', { name: 'Search cards' })).toBeVisible();
      const covered = await header(page).evaluate((element) => {
        const search = element.querySelector('[data-testid="view-search"]');
        const onTop = (selector: string): boolean => {
          const box = element.querySelector(selector)?.getBoundingClientRect();
          if (!box) return false;
          const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
          return search?.contains(hit) ?? false;
        };
        return {
          wide: search?.getBoundingClientRect().width === element.getBoundingClientRect().width,
          views: onTop('[data-tool="views"]'),
          filter: onTop('[data-tool="filter"]') || !element.querySelector('[data-tool="filter"]'),
        };
      });
      expect(covered).toEqual({ wide: true, views: true, filter: true });
    }

    await field.pressSequentially('proj');
    await expect(cards(page)).toHaveCount(2);
    await field.pressSequentially('ect b');
    await expect(cards(page)).toHaveCount(1);
    await expect(cards(page)).toContainText('Project budget approval');
    // Search is the reader's own: nothing of it is stored in the view.
    expect(JSON.stringify(await storedView(page))).not.toContain('project b');

    if (folded) {
      await header(page).getByRole('button', { name: 'Close search' }).click();
      await expect(field).toBeHidden();
      await expect(tool(page, 'search')).toBeFocused();
      await expect(tool(page, 'views')).toBeVisible();
    } else {
      await field.press('Escape');
    }
    await expect(field).toHaveValue('');
    await expect(cards(page)).toHaveCount(5);

    // Another view starts without the search.
    if (folded) await tool(page, 'search').click();
    await field.fill('scope');
    await expect(cards(page)).toHaveCount(1);
    if (folded) {
      // While the field covers the row, what is under it is out of reach.
      expect(
        await tool(page, 'more').evaluate((element) => element.closest('[inert]') !== null),
      ).toBe(true);
      await page.goBack();
    } else {
      await showView(page, 'Progress Tracker');
    }
    await expect(field).toHaveValue('');
  });
});

test.describe('a compact page', () => {
  test('menus are sheets, and the back button closes them', async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== 'mobile', 'the compact stage');
    await open(page);
    await showView(page, 'Task Overview');
    const address = page.url();

    // The list of views.
    await tool(page, 'views').click();
    await expect(sheet(page)).toBeVisible();
    await expect(sheet(page).getByRole('menuitem', { name: 'Task Overview' })).toBeVisible();
    await page.goBack();
    await expect(sheet(page)).toHaveCount(0);
    expect(page.url()).toBe(address);

    // The filter. Nothing in it has the focus: the keyboard waits to be asked for.
    await tool(page, 'filter').click();
    await expect(sheet(page)).toBeVisible();
    await expect(sheet(page)).toContainText('Filter');
    await expect(sheet(page)).toBeFocused();
    const title = await addCondition(page, 'Title', 'contains');
    // A sheet over the sheet, for the list of properties, came and went.
    await expect(sheet(page)).toHaveCount(1);
    // The line is folded in two: the value is under the property, not beside it.
    const boxes = await title.evaluate((row) => ({
      property: row.querySelector('[data-slot="filter-property"]')?.getBoundingClientRect().bottom,
      value: row.querySelector('[data-slot="filter-value"]')?.getBoundingClientRect().top,
    }));
    expect(boxes.value).toBeGreaterThanOrEqual(boxes.property ?? Infinity);
    // What a finger presses is 44 pixels high.
    for (const slot of ['filter-property', 'filter-condition']) {
      const box = await title.locator(`[data-slot="${slot}"]`).boundingBox();
      expect(box?.height).toBeGreaterThanOrEqual(44);
    }
    const text = title.getByRole('textbox', { name: 'Value for Title' });
    expect(await text.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBe(
      16,
    );
    await text.fill('scope');
    await text.press('Enter');
    await expect(cards(page)).toHaveCount(1);
    await page.goBack();
    await expect(sheet(page)).toHaveCount(0);
    expect(page.url()).toBe(address);
    await expect(cards(page)).toHaveCount(1);

    // "…" and, from it, the sort: one sheet takes the place of the other, and back closes it.
    await tool(page, 'more').click();
    await expect(sheet(page)).toContainText('View menu');
    await sheet(page).locator('[data-action="sort"]').click();
    await expect(sortEditor(page)).toBeVisible();
    await expect(sheet(page)).toHaveCount(1);
    await addSort(page, 'Title');
    await page.goBack();
    await expect(sheet(page)).toHaveCount(0);
    expect(page.url()).toBe(address);
    // One more step back leaves the view, as it would have without the sheets.
    await page.goBack();
    await expect(page).not.toHaveURL(address);
    await expect(header(page)).toBeVisible();

    // "Done" closes a sheet too.
    await tool(page, 'filter').click();
    await sheet(page).getByRole('button', { name: 'Done' }).click();
    await expect(sheet(page)).toHaveCount(0);
  });
});

test.describe('a board the reader may only look at', () => {
  test('has the views and search, and nothing that changes the view', async ({ page }) => {
    await open(page);
    await open(page, await boardAddress(page, 'Company Goals & OKRs'));
    for (const name of ['properties', 'group', 'filter', 'sort', 'more', 'add-view', 'rename']) {
      await expect(tool(page, name)).toHaveCount(0);
    }
    await expect(page.getByTestId('new-card')).toHaveCount(0);

    await showView(page, 'Departments');
    expect((await storedView(page)).title).toBe('Departments');
    if ((await layout(page)) === 'folded') {
      // The list switches views and offers nothing else.
      await tool(page, 'views').click();
      await expect(page.getByRole('menuitem')).toHaveCount(3);
      await closeTop(page);
      await tool(page, 'search').click();
    }
    const before = await cards(page).count();
    expect(before).toBeGreaterThan(1);
    await header(page).getByRole('searchbox', { name: 'Search cards' }).fill('backlog');
    await expect.poll(() => cards(page).count()).toBeLessThan(before);
    await expect(cards(page).first()).toContainText('Reduce bug backlog');
  });
});

test.describe('while the window changes size', () => {
  test('what is open stays open, and what was typed stays typed', async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'starts wide and goes through every stage');
    await open(page);
    await showView(page, 'Task Overview');
    const field = header(page).getByRole('searchbox', { name: 'Search cards' });
    await field.fill('e');
    await expect(cards(page)).toHaveCount(5);
    await field.fill('project');
    await expect(cards(page)).toHaveCount(2);

    // The filter open in the whole row, with a text typed and not yet saved.
    await openTool(page, 'filter');
    const title = await addCondition(page, 'Title', 'contains');
    const text = title.getByRole('textbox', { name: 'Value for Title' });
    await text.pressSequentially('scope');
    expect((await storedView(page)).fields.filter.filters).toEqual([
      { propertyId: 'title', condition: 'contains', values: [] },
    ]);

    // Narrower: the row folds. The popover is the same one, and so is the field in it.
    await text.evaluate((element) => {
      (window as unknown as { kept: Element }).kept = element;
    });
    await resize(page, 1100, 720);
    await expect(header(page)).toHaveAttribute('data-layout', 'folded');
    await expect(filterEditor(page)).toBeVisible();
    await expect(page.locator('[data-slot="popover-content"]')).toHaveCount(1);
    await expect(text).toHaveValue('scope');
    await expect(text).toBeFocused();
    expect(
      await text.evaluate((element) => (window as unknown as { kept: Element }).kept === element),
    ).toBe(true);
    // The search text is there too, and the folded row shows it.
    await expect(field).toHaveValue('project');
    await expect(field).toBeVisible();

    // Compact: the popover becomes a sheet. It is built anew; the text was saved on the way.
    await resize(page, 500, 720);
    await expect(sheet(page)).toBeVisible();
    await expect(page.locator('[data-slot="popover-content"]')).toHaveCount(0);
    await expect(filterEditor(page)).toBeVisible();
    await expect(text).toHaveValue('scope');
    // The field is another element than before, and the focus did not come along:
    // what the shared surface would have to do differently to keep those is in
    // docs/fragments/view-header-decisions.md.
    expect(
      await text.evaluate((element) => (window as unknown as { kept: Element }).kept === element),
    ).toBe(false);
    await expect(text).not.toBeFocused();
    await expect(field).toHaveValue('project');
    await expect(cards(page)).toHaveCount(1);
    expect((await storedView(page)).fields.filter.filters).toEqual([
      { propertyId: 'title', condition: 'contains', values: ['scope'] },
    ]);

    // And back to a popover, and to the whole row.
    await resize(page, 700, 720);
    await expect(sheet(page)).toHaveCount(0);
    await expect(page.locator('[data-slot="popover-content"]')).toHaveCount(1);
    await expect(text).toHaveValue('scope');
    await resize(page, 1280, 720);
    await expect(header(page)).toHaveAttribute('data-layout', 'row');
    await expect(filterEditor(page)).toBeVisible();
    await expect(field).toHaveValue('project');
    // The popover is next to the button it belongs to.
    const gap = await page.evaluate(() => {
      const button = document.querySelector('[data-tool="filter"]')?.getBoundingClientRect();
      const popover = document
        .querySelector('[data-slot="popover-content"]')
        ?.getBoundingClientRect();
      return button && popover ? popover.top - button.bottom : undefined;
    });
    expect(gap).toBeGreaterThanOrEqual(0);
    expect(gap).toBeLessThan(16);
  });

  test('a tool opened from "…" stays open, next to "…" or to its own button', async ({
    page,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'starts wide and goes through every stage');
    await open(page);
    await resize(page, 1100, 720);
    await openTool(page, 'sort');
    await expect(sortEditor(page)).toBeVisible();
    const anchored = (name: string): Promise<{ below: number; across: number } | undefined> =>
      page.evaluate((selector) => {
        const button = document.querySelector(selector)?.getBoundingClientRect();
        const popover = document
          .querySelector('[data-slot="popover-content"]')
          ?.getBoundingClientRect();
        if (!button || !popover) return undefined;
        return {
          below: popover.top - button.bottom,
          across: Math.max(button.left - popover.right, popover.left - button.right),
        };
      }, `[data-tool="${name}"]`);
    // Under "…", which is where it was opened from.
    await expect.poll(async () => (await anchored('more'))?.below).toBeLessThan(16);
    expect((await anchored('more'))?.across).toBeLessThanOrEqual(0);

    // The pane widens: the sort has its own button now, and the popover is under that.
    await resize(page, 1300, 720);
    await expect(header(page)).toHaveAttribute('data-layout', 'row');
    await expect(sortEditor(page)).toBeVisible();
    await expect(tool(page, 'sort')).toBeVisible();
    await expect.poll(async () => (await anchored('sort'))?.across).toBeLessThanOrEqual(0);
    expect((await anchored('sort'))?.below).toBeLessThan(16);

    // The name of a view being typed outlives the surface it is typed on.
    await closeTop(page);
    await resize(page, 700, 720);
    await viewCommand(page, 'rename');
    const name = page.getByRole('textbox', { name: 'View name' });
    await name.fill('Half typed');
    await resize(page, 500, 720);
    await expect(sheet(page)).toBeVisible();
    await expect(name).toHaveValue('Half typed');
    await resize(page, 700, 720);
    await expect(sheet(page)).toHaveCount(0);
    await expect(name).toHaveValue('Half typed');
    expect((await storedView(page)).title).toBe('Progress Tracker');
    await name.press('Enter');
    await expect.poll(async () => (await storedView(page)).title).toBe('Half typed');
  });
});

/**
 * The pictures of docs/screens/view-header/: every scene at the stage of the
 * project that runs, light and dark. Only with `FVOCI_SCREENS=1`:
 *
 *   FVOCI_SCREENS=1 pnpm exec playwright test e2e/view-header.spec.ts -g pictures
 */
test.describe('pictures', () => {
  test.skip(process.env.FVOCI_SCREENS !== '1', 'only when asked for');

  const STAGES: Record<string, string> = {
    mobile: 'compact',
    middle: 'medium',
    desktop: 'expanded',
  };
  const directory = path.resolve(import.meta.dirname, '../docs/screens/view-header');

  type Scene = (page: Page) => Promise<void>;
  const SCENES: Record<string, Scene> = {
    header: async () => undefined,
    views: async (page) => {
      if ((await layout(page)) === 'row') await tool(page, 'add-view').click();
      else await tool(page, 'views').click();
      await expect(page.getByRole('menuitem').first()).toBeVisible();
    },
    more: async (page) => {
      await tool(page, 'more').click();
      await expect(page.getByRole('menuitem').first()).toBeVisible();
    },
    properties: async (page) => {
      await openTool(page, 'properties');
      await expect(page.getByTestId('properties-panel')).toBeVisible();
    },
    group: async (page) => {
      await openTool(page, 'group');
      await expect(page.getByRole('menuitem').first()).toBeVisible();
    },
    filter: async (page) => {
      const all = await properties(page);
      await patchView(page, {
        filter: {
          operation: 'and',
          filters: [
            {
              propertyId: all.Priority?.id,
              condition: 'includes',
              values: [all.Priority?.options['1. High 🔥'], all.Priority?.options['2. Medium']],
            },
            { propertyId: 'title', condition: 'contains', values: ['project'] },
            { propertyId: all['Date Created']?.id, condition: 'isBefore', values: ['2026-12-24'] },
            { propertyId: all['Due Date']?.id, condition: 'isNotSet', values: [] },
          ],
        },
      });
      await openTool(page, 'filter');
      await expect(filterRows(page)).toHaveCount(4);
    },
    sort: async (page) => {
      const all = await properties(page);
      await patchView(page, {
        sortOptions: [
          { propertyId: all.Priority?.id, reversed: false },
          { propertyId: '__title', reversed: true },
        ],
      });
      await openTool(page, 'sort');
      await expect(sortEditor(page).locator('[data-sort]')).toHaveCount(2);
    },
    search: async (page) => {
      if ((await layout(page)) === 'folded') await tool(page, 'search').click();
      await header(page).getByRole('searchbox').fill('project');
      await expect(cards(page)).toHaveCount(2);
    },
  };

  for (const [scene, arrange] of Object.entries(SCENES)) {
    for (const scheme of ['light', 'dark'] as const) {
      test(`${scene}, ${scheme}`, async ({ page }, testInfo) => {
        const stage = STAGES[testInfo.project.name];
        if (!stage) throw new Error(`no stage for ${testInfo.project.name}`);
        await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
        await open(page);
        await showView(page, 'Task Overview');
        await expect(cards(page)).toHaveCount(5);
        await arrange(page);
        await page.waitForLoadState('networkidle');
        await page.evaluate(() => document.fonts.ready);
        // Let a sheet or a popover finish arriving.
        await page.waitForTimeout(400);
        mkdirSync(directory, { recursive: true });
        await page.screenshot({
          path: path.join(directory, `${scene}-${stage}-${scheme}.png`),
          // One pixel of the picture per pixel of the layout: the files stay small.
          scale: 'css',
        });
      });
    }
  }
});
