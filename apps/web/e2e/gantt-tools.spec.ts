/**
 * What is done to a Gantt view on the in-memory backend: grouping it, its
 * zoom, the cards without a date, and how it draws and scrolls a board of
 * 2,000 cards (the figures are printed, for a person to read).
 */
import { expect, test, type Page, type TestInfo } from '@playwright/test';

import {
  arrows,
  bar,
  bars,
  dayScale,
  gantt,
  NOW,
  openGantt,
  openRelated,
  scroller,
  storedFields,
  type Dev,
} from './gantt.support.ts';
import { dialog, fitPane, paneOf } from './kanban.support.ts';

test.use({ timezoneId: 'Asia/Seoul', locale: 'en-US' });

interface Option {
  id: string;
  value: string;
}
interface Select {
  id: string;
  name: string;
  options: Option[];
}

/** The first select property of the open board, and the option each dated card has. */
async function grouping(page: Page): Promise<{ select: Select; optionOf: Record<string, string> }> {
  return page.evaluate(() => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    const select = store.board.cardProperties.find((property) => property.type === 'select');
    if (!select) throw new Error('the board has no select property');
    const cards = store.cards as unknown as {
      title: string;
      fields: { properties: Record<string, unknown> };
    }[];
    return {
      select: select as unknown as Select,
      optionOf: Object.fromEntries(
        cards.map((card) => {
          const value = card.fields.properties[select.id];
          return [card.title, typeof value === 'string' ? value : ''];
        }),
      ),
    };
  });
}

/** Chooses a line of "group by" in the header, whichever way the header is laid out. */
async function chooseGroup(page: Page, name: string): Promise<void> {
  const header = page.getByTestId('view-header');
  if ((await header.getAttribute('data-layout')) === 'row') {
    await header.locator('[data-tool="group"]').click();
  } else {
    await header.locator('[data-tool="more"]').click();
    await page.locator('[role="menuitem"][data-action="group"]').click();
  }
  await page.getByRole('menuitem', { name, exact: true }).click();
}

test('grouped as a table is: a heading before the cards of each group, the arrows across them', async ({
  page,
}, info) => {
  await openRelated(page, info);
  await expect(gantt(page).locator('[data-group]')).toHaveCount(0);
  const { select, optionOf } = await grouping(page);
  await chooseGroup(page, select.name);
  expect((await storedFields(page)).groupById).toBe(select.id);

  // One heading for the cards without a value and one for each option, as in a table.
  const headings = gantt(page).locator('[data-group]');
  await expect(headings).toHaveCount(select.options.length + 1);
  const dated = ['Drag cards', 'Share a board', 'Create a new card', 'Filter and sort cards'];
  for (const option of [{ id: '', value: `No ${select.name}` }, ...select.options]) {
    const heading = gantt(page).locator(`[data-group="${option.id}"]`);
    const count = dated.filter((title) => (optionOf[title] ?? '') === option.id).length;
    await expect(heading.locator('[data-slot="group-name"]')).toHaveText(option.value);
    await expect(heading.locator('[data-slot="group-count"]')).toContainText(String(count));
  }
  // Each card is under the heading of its option: the rows follow the headings down the page.
  const order = await gantt(page)
    .locator('[data-group], [data-row]')
    .evaluateAll((rows) =>
      rows.map((row) =>
        row.hasAttribute('data-group')
          ? `group:${row.getAttribute('data-group') ?? ''}`
          : (row.querySelector('[data-row-title]')?.textContent ?? '').trim(),
      ),
    );
  let under = '';
  for (const line of order) {
    if (line.startsWith('group:')) under = line.slice('group:'.length);
    else expect(optionOf[line] ?? '', `"${line}" under its group`).toBe(under);
  }
  expect(order.filter((line) => !line.startsWith('group:')).sort()).toEqual([...dated].sort());
  // The arrows are still one for each edge, whatever group their cards are in.
  await expect(arrows(page)).toHaveCount(3);
  await expect(gantt(page)).toHaveAttribute('data-edges', '3');
  // A heading is not a stop of the keys that walk the bars.
  await bars(page).first().focus();
  for (let step = 0; step < 3; step += 1) await page.keyboard.press('ArrowDown');
  await expect(bars(page).last()).toBeFocused();

  await chooseGroup(page, 'Ungroup');
  await expect(headings).toHaveCount(0);
  await expect(bars(page)).toHaveCount(4);
});

test('the zoom steps through days, weeks and months, and the view keeps it', async ({
  page,
}, info) => {
  await openGantt(page, info);
  const line = page.getByTestId('gantt-today-line');
  const step = page.getByTestId('gantt-zoom');
  const zoomIn = page.getByTestId('gantt-zoom-in');
  const zoomOut = page.getByTestId('gantt-zoom-out');
  /** How many days a bar is wide by the marks of the axis, and whether it lies on its first day. */
  async function onDays(title: string, first: string, days: number): Promise<void> {
    await bar(page, title).scrollIntoViewIfNeeded();
    const scale = await dayScale(page);
    const box = await bar(page, title).boundingBox();
    if (!box) throw new Error(`"${title}" has no bar`);
    expect(Math.abs(box.x - scale.x(first))).toBeLessThan(1);
    expect(Math.abs(box.width - days * scale.width)).toBeLessThan(1);
  }

  await expect(step).toContainText('Zoom: Weeks');
  await zoomIn.click();
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'day');
  await expect(step).toContainText('Zoom: Days');
  expect((await storedFields(page)).ganttZoom).toBe('day');
  // Today, which was in sight, still is.
  await expect(line).toBeInViewport();
  // A mark for each day, under the name of the month.
  const first = (await gantt(page).getAttribute('data-first')) ?? '';
  const last = (await gantt(page).getAttribute('data-last')) ?? '';
  const days = Math.round((Date.parse(last) - Date.parse(first)) / 86_400_000) + 1;
  await expect(gantt(page).locator('[data-tick]')).toHaveCount(days);
  await expect(gantt(page).locator('[data-upper="2026-11-01"]')).toHaveText('November 2026');
  await onDays('Share a board', '2026-11-12', 12);
  // The closest step: the button stays and does nothing.
  await expect(zoomIn).toHaveAttribute('aria-disabled', 'true');
  // Pressed all the same (the test's own click would wait for the button to be enabled).
  await zoomIn.click({ force: true });
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'day');

  await zoomOut.click();
  await zoomOut.click();
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'month');
  await expect(step).toContainText('Zoom: Months');
  expect((await storedFields(page)).ganttZoom).toBe('month');
  await expect(zoomOut).toHaveAttribute('aria-disabled', 'true');
  await expect(line).toBeInViewport();
  // Months under years, on whole months.
  expect(((await gantt(page).getAttribute('data-first')) ?? '').slice(8)).toBe('01');
  await expect(gantt(page).locator('[data-upper]').first()).toHaveText('2026');
  await expect(gantt(page).locator('[data-tick="2026-11-01"]')).toHaveText('Nov');
  // A day is a fifth of a rem wide: the bar of one day is widened to be seen.
  const narrow = await bar(page, 'Drag cards').boundingBox();
  expect(Math.round(narrow?.width ?? 0)).toBe(8);
  // Back in: the days that were in the middle before zooming out are there again.
  await zoomIn.click();
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'week');
  await expect(line).toBeInViewport();
  await zoomOut.click();
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'month');
  // One undo takes one step back.
  await page.evaluate(() =>
    (
      window as unknown as { fvociDev: { workspace: { undoLast(): Promise<void> } } }
    ).fvociDev.workspace.undoLast(),
  );
  await expect(gantt(page)).toHaveAttribute('data-zoom', 'week');
});

test('the cards without a date are counted above the chart and listed on asking', async ({
  page,
}, info) => {
  await openGantt(page, info);
  const total = await page.evaluate(
    () =>
      (
        (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board.cards as unknown as {
          fields: { isTemplate: boolean };
        }[]
      ).filter((card) => !card.fields.isTemplate).length,
  );
  const button = page.getByTestId('gantt-undated');
  await expect(button).toHaveAccessibleName(`No date: ${total - 4}`);
  await expect(button).toHaveAttribute('aria-expanded', 'false');
  await button.click();
  const list = page.getByTestId('undated-list');
  await expect(list.locator('[data-card]')).toHaveCount(total - 4);
  await expect(list).toContainText('Create a new view');
  await expect(list).not.toContainText('Share a board');
  await expect(list).toContainText('These cards have no Due Date');

  // A card opened from the list gives the focus back to "No date" when it closes.
  await list.getByRole('link', { name: 'Create a new view' }).click();
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page).locator('[data-card-title]')).toHaveValue('Create a new view');
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toHaveCount(0);
  await expect(button).toBeFocused();

  // Where the list is a column beside the chart, Escape in it closes it.
  if ((await page.getByTestId('undated-panel').count()) > 0) {
    await page.getByTestId('undated-close').focus();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('undated-panel')).toHaveCount(0);
    await expect(button).toBeFocused();
  }
  await expect(list).toHaveCount(0);
});

// --------------------------------------------------------- 2,000 cards

const MANY = 2000;

interface Backend {
  latency: number;
  teams(): Promise<{ id: string }[]>;
  boards(teamId: string): Promise<{ id: string; title: string }[]>;
  board(boardId: string): Promise<{ cardProperties: { id: string; name: string }[] }>;
  patchBoard(boardId: string, patch: Record<string, unknown>): Promise<unknown>;
  createCard(boardId: string, draft: Record<string, unknown>): Promise<{ id: string }>;
  createView(boardId: string, draft: Record<string, unknown>): Promise<{ id: string }>;
}

/**
 * Gives a board the page is not showing 2,000 dated cards, each coming after
 * the one before it, and a Gantt view of them; then shows that view, by the
 * app's own way of going to an address. Answers how long the view took to
 * be drawn, in milliseconds.
 */
async function openMany(page: Page, info: TestInfo): Promise<number> {
  await page.clock.setFixedTime(NOW);
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  await fitPane(page, paneOf(info));
  const path = await page.evaluate(async (count) => {
    const backend = (window as unknown as { fvociDev: { backend: Backend } }).fvociDev.backend;
    backend.latency = 0;
    const teams = await backend.teams();
    const boards = await backend.boards(teams[0]?.id ?? '');
    const boardId = boards.find((board) => board.title === 'Sprint Planner')?.id ?? '';
    const due = (await backend.board(boardId)).cardProperties.find(
      (property) => property.name === 'Due Date',
    );
    if (!due) throw new Error('no board "Sprint Planner" with a Due Date');
    await backend.patchBoard(boardId, {
      updatedCardProperties: [{ id: 'after', name: 'After', type: 'relation', options: [] }],
    });
    const day = (offset: number): string =>
      new Date(Date.UTC(2026, 9, 1) + offset * 86_400_000).toISOString().slice(0, 10);
    let before: string | undefined;
    for (let at = 0; at < count; at += 1) {
      // Eight in nine are of several days; some one in nine starts before the one it comes after ends.
      const from = (at * 7) % 240;
      const made = await backend.createCard(boardId, {
        title: `Task ${String(at + 1)}`,
        properties: {
          [due.id]: { from: day(from), to: day(from + (at % 9)) },
          ...(before ? { after: [before] } : {}),
        },
      });
      before = made.id;
    }
    const view = await backend.createView(boardId, {
      title: 'Many',
      fields: {
        viewType: 'gantt',
        dateDisplayPropertyId: due.id,
        dependencyPropertyId: 'after',
        ganttZoom: 'week',
        sortOptions: [],
        visiblePropertyIds: ['__title'],
        visibleOptionIds: [],
        hiddenOptionIds: [],
        collapsedOptionIds: [],
        filter: { operation: 'and', filters: [] },
        cardOrder: [],
        columnWidths: {},
        columnCalculations: {},
        kanbanCalculations: {},
        defaultTemplateId: '',
      },
    });
    return `/board/${boardId}/${view.id}`;
  }, MANY);
  // Timed on the page, from the change of address to the second frame after the last bar is there.
  const drawn = await page.evaluate(
    ([address, count]) =>
      new Promise<number>((resolve) => {
        const started = performance.now();
        history.pushState(null, '', address);
        window.dispatchEvent(new PopStateEvent('popstate'));
        const look = (): void => {
          if (document.querySelectorAll('[data-bar]').length < count) {
            requestAnimationFrame(look);
            return;
          }
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              resolve(Math.round(performance.now() - started));
            });
          });
        };
        requestAnimationFrame(look);
      }),
    [path, MANY] as const,
  );
  await expect(gantt(page)).toBeVisible();
  expect(await bars(page).count()).toBeGreaterThanOrEqual(MANY);
  return drawn;
}

/** Scrolls a box a step a frame and answers the time each frame took, in milliseconds. */
async function scrollFrames(
  page: Page,
  axis: 'scrollTop' | 'scrollLeft',
  step: number,
  frames: number,
): Promise<number[]> {
  return scroller(page).evaluate(
    (element, [property, by, count]) =>
      new Promise<number[]>((resolve) => {
        const times: number[] = [];
        let last = performance.now();
        const frame = (now: number): void => {
          times.push(now - last);
          last = now;
          if (times.length > count) {
            // The first interval is the wait for the first frame, not a frame.
            resolve(times.slice(1));
            return;
          }
          element[property] += by;
          requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
      }),
    [axis, step, frames] as const,
  );
}

/** The middle, the 95th of a hundred, and the longest of some times, rounded to a tenth. */
function summary(times: number[]): { median: number; p95: number; max: number } {
  const sorted = [...times].sort((a, b) => a - b);
  const at = (part: number): number =>
    Math.round((sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * part))] ?? 0) * 10) /
    10;
  return { median: at(0.5), p95: at(0.95), max: at(1) };
}

test('a board of 2,000 cards draws every bar and arrow, and scrolls in frames that are measured', async ({
  page,
}, info) => {
  test.setTimeout(180_000);
  const drawn = await openMany(page, info);
  await expect(gantt(page)).toHaveAttribute('data-edges', String(MANY - 1));
  await expect(arrows(page)).toHaveCount(MANY - 1);
  expect(Number(await gantt(page).getAttribute('data-late'))).toBeGreaterThan(100);

  const down = summary(await scrollFrames(page, 'scrollTop', 90, 120));
  const across = summary(await scrollFrames(page, 'scrollLeft', 40, 120));
  expect(await scroller(page).evaluate((element) => element.scrollTop)).toBeGreaterThan(9000);

  // One card renamed: how long the page is busy before it can draw again.
  const renamed = await page.evaluate(async () => {
    const store = (
      window as unknown as {
        fvociDev: {
          workspace: {
            board: {
              cards: { id: string; title: string }[];
              setCardTitle(cardId: string, title: string): Promise<void>;
            };
          };
        };
      }
    ).fvociDev.workspace.board;
    const card = store.cards.find((each) => each.title === 'Task 1000');
    if (!card) throw new Error('no card to rename');
    const started = performance.now();
    const written = store.setCardTitle(card.id, 'Task 1000, renamed');
    await new Promise((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(resolve));
    });
    const busy = performance.now() - started;
    await written;
    return Math.round(busy);
  });
  await expect(bar(page, 'Task 1000, renamed')).toHaveCount(1);

  const figures = {
    project: info.project.name,
    cards: MANY,
    arrows: MANY - 1,
    drawnMs: drawn,
    scrollDownFrameMs: down,
    scrollAcrossFrameMs: across,
    renameOneCardMs: renamed,
  };
  // Printed for a person to read, with the output of the run.
  process.stdout.write(`[gantt 2000] ${JSON.stringify(figures)}\n`);
  info.annotations.push({ type: 'gantt 2000', description: JSON.stringify(figures) });
  // Generous bounds: the figures are for reading, the bounds catch a chart that stopped scrolling.
  expect(down.median).toBeLessThan(100);
  expect(across.median).toBeLessThan(100);
  expect(renamed).toBeLessThan(2000);
});
