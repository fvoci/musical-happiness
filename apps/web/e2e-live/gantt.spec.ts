/**
 * The Gantt view on the real server: a view of the kind `gantt` that the
 * server keeps, dated cards as bars on their days, and what the view is
 * placed by chosen on the page and still there after a reload.
 *
 * The view is made through the API: the page's own "add a view" is not yet
 * wired to the server (`createView` answers `not_wired`, src/lib/api). The
 * server does not check the kind of a view or the members of its `fields`,
 * so a Gantt chart is a view like any other to it.
 */
import { expect, test, type APIRequestContext, type Locator, type Page } from '@playwright/test';

import {
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US', timezoneId: 'Asia/Seoul' });

const bar = (page: Page, title: string): Locator =>
  page
    .getByTestId('gantt')
    .locator('[data-row]')
    .filter({ has: page.locator('[data-row-title]', { hasText: title }) })
    .locator('[data-bar]');

/** Makes a Gantt view through the API and answers its identifier. */
async function makeGantt(
  api: APIRequestContext,
  boardId: string,
  fields: Record<string, unknown>,
): Promise<string> {
  const made = await api.post(`/api/v1/boards/${boardId}/blocks`, {
    data: [
      {
        boardId,
        type: 'view',
        title: 'Gantt view',
        createAt: 1,
        updateAt: 1,
        fields: {
          viewType: 'gantt',
          sortOptions: [],
          visiblePropertyIds: ['__title'],
          visibleOptionIds: [],
          hiddenOptionIds: [],
          collapsedOptionIds: [],
          cardOrder: [],
          filter: { operation: 'and', filters: [] },
          ...fields,
        },
      },
    ],
  });
  expect(made.ok(), await made.text()).toBe(true);
  const [view] = (await made.json()) as Schemas['Block'][];
  return view?.id ?? '';
}

/** The fields of a view as the server keeps them. */
async function storedFields(
  api: APIRequestContext,
  boardId: string,
  viewId: string,
): Promise<Record<string, unknown>> {
  const views = (await (
    await api.get(`/api/v1/boards/${boardId}/blocks`, { params: { type: 'view' } })
  ).json()) as Schemas['Block'][];
  return views.find((each) => each.id === viewId)?.fields ?? {};
}

test('a Gantt view of the server draws dated cards as bars, and keeps its "display by"', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('gantt');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, { title: 'Harvest plan' });
  // A second date, after "Due": the one the page will choose.
  const added = await api.patch(`/api/v1/boards/${board.boardId}`, {
    data: { updatedCardProperties: [{ id: 'start', name: 'Start', type: 'date', options: [] }] },
  });
  expect(added.ok()).toBe(true);
  for (const [title, due, start] of [
    ['Pick apples', { from: '2026-11-12' }, { from: '2026-11-03' }],
    ['Press cider', { from: '2026-11-12', to: '2026-11-17' }, { from: '2026-11-04' }],
  ] as const) {
    const answer = await api.post(`/api/v1/boards/${board.boardId}/cards`, {
      data: { title, properties: { due, start } },
    });
    expect(answer.ok()).toBe(true);
  }
  // A chart that names no date yet: it is placed by the first, "Due".
  const viewId = await makeGantt(api, board.boardId, {});

  await page.clock.setFixedTime(new Date('2026-11-10T00:00:00Z'));
  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${viewId}`);
  await signInOnPage(page, owner);

  const gantt = page.getByTestId('gantt');
  await expect(gantt).toHaveAttribute('data-zoom', 'week');
  await expect(gantt.locator('[data-bar]')).toHaveCount(2);
  await expect(bar(page, 'Pick apples')).toHaveAccessibleName('Pick apples, November 12');
  await expect(bar(page, 'Press cider')).toHaveAccessibleName('Press cider, November 12 – 17');
  /** How many days a bar is wide, by the marks of the axis (a week each). */
  const daysWide = async (title: string): Promise<number> => {
    const week = await gantt.locator('[data-tick]').nth(1).boundingBox();
    const box = await bar(page, title).boundingBox();
    if (!week || !box) throw new Error('nothing to measure');
    return Math.round(box.width / (week.width / 7));
  };
  expect(await daysWide('Press cider')).toBe(6);

  const tool = page.getByTestId('view-header').locator('[data-tool="display-by"]');
  await expect(tool).toHaveAccessibleName('Display by: Due');
  expect((await storedFields(api, board.boardId, viewId)).dateDisplayPropertyId).toBeUndefined();
  await tool.click();
  await page.locator('[role="menuitem"][data-action="start"]').click();
  await expect
    .poll(async () => (await storedFields(api, board.boardId, viewId)).dateDisplayPropertyId)
    .toBe('start');
  await expect(bar(page, 'Press cider')).toHaveAccessibleName('Press cider, November 4');

  // After a reload the view is still a Gantt chart, placed by "Start" as the server keeps it.
  await page.reload();
  await expect(gantt.locator('[data-bar]')).toHaveCount(2);
  await expect(tool).toHaveAccessibleName('Display by: Start');
  await expect(bar(page, 'Pick apples')).toHaveAccessibleName('Pick apples, November 3');
  expect(await daysWide('Press cider')).toBe(1);
  expect((await storedFields(api, board.boardId, viewId)).viewType).toBe('gantt');
  expect(errors).toEqual([]);
  await api.dispose();
});
