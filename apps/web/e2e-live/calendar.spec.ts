/**
 * The calendar view on the real server: a view of the kind `calendar` that
 * the server keeps, dated cards on their days, and the date it is placed by
 * chosen on the page and still there after a reload.
 *
 * The view is made through the API: the page's own "add a view" is not yet
 * wired to the server (`createView` answers `not_wired`, src/lib/api). The
 * server does not check the kind of a view, so a calendar is a view like any
 * other to it. The choice of "display by" goes through the page, as a change
 * of the view's `dateDisplayPropertyId`.
 */
import { expect, test } from '@playwright/test';

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

test('a calendar view of the server shows dated cards, and keeps its "display by"', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('calendar');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, { title: 'Harvest calendar' });
  // A second date, after "Due": the one the page will choose. Not the first,
  // which a view that names no date falls back to.
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
  // A calendar that names no date yet: it is placed by the first, "Due".
  const made = await api.post(`/api/v1/boards/${board.boardId}/blocks`, {
    data: [
      {
        boardId: board.boardId,
        type: 'view',
        title: 'Calendar view',
        createAt: 1,
        updateAt: 1,
        fields: {
          viewType: 'calendar',
          sortOptions: [],
          visiblePropertyIds: ['__title'],
          visibleOptionIds: [],
          hiddenOptionIds: [],
          collapsedOptionIds: [],
          cardOrder: [],
          filter: { operation: 'and', filters: [] },
        },
      },
    ],
  });
  expect(made.ok()).toBe(true);
  const [view] = (await made.json()) as Schemas['Block'][];
  const viewId = view?.id ?? '';
  const stored = async (): Promise<Record<string, unknown>> => {
    const views = (await (
      await api.get(`/api/v1/boards/${board.boardId}/blocks`, { params: { type: 'view' } })
    ).json()) as Schemas['Block'][];
    return views.find((each) => each.id === viewId)?.fields ?? {};
  };

  await page.clock.setFixedTime(new Date('2026-11-10T00:00:00Z'));
  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${viewId}`);
  await signInOnPage(page, owner);

  const calendar = page.getByTestId('calendar');
  await expect(calendar).toHaveAttribute('data-layout', 'bars');
  await expect(page.getByTestId('calendar-period')).toHaveText('November 2026');
  const grid = page.getByTestId('month-grid');
  await expect(grid.locator('[data-bar]').filter({ hasText: 'Pick apples' })).toHaveCount(1);
  // Thursday the 12th to Tuesday the 17th: two weeks, two bars.
  await expect(grid.locator('[data-bar]').filter({ hasText: 'Press cider' })).toHaveCount(2);

  const tool = page.getByTestId('view-header').locator('[data-tool="display-by"]');
  await expect(tool).toHaveAccessibleName('Display by: Due');
  expect((await stored()).dateDisplayPropertyId).toBeUndefined();
  await tool.click();
  await page.locator('[role="menuitem"][data-action="start"]').click();
  await expect.poll(async () => (await stored()).dateDisplayPropertyId).toBe('start');
  const week = (date: string) => grid.locator(`[data-week="${date}"] [data-bar]`);
  await expect(week('2026-11-01').filter({ hasText: 'Pick apples' })).toHaveCount(1);

  // After a reload the view is still a calendar, placed by "Start" as the server keeps it.
  await page.reload();
  await expect(calendar).toHaveAttribute('data-layout', 'bars');
  await expect(tool).toHaveAccessibleName('Display by: Start');
  await expect(week('2026-11-01').filter({ hasText: 'Pick apples' })).toHaveCount(1);
  await expect(week('2026-11-01').filter({ hasText: 'Press cider' })).toHaveCount(1);
  await expect(week('2026-11-08')).toHaveCount(0);
  expect((await stored()).viewType).toBe('calendar');
  expect(errors).toEqual([]);
  await api.dispose();
});
