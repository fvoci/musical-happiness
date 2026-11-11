/**
 * The calendar view on the real server: made on the page from the list of
 * kinds, dated cards on their days, and the date it is placed by chosen on
 * the page; after a reload the view is still a calendar placed by that date,
 * as the server keeps it. The server does not check the kind of a view, so a
 * calendar is a view like any other to it; "display by" is the view's
 * `dateDisplayPropertyId`. A date changed on the calendar is one change of a
 * card property, kept like any other.
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
  // A second date, after "Due": the one the page will choose. Not the first, which a new
  // calendar is placed by, so that the reload shows the choice and not the start.
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
  /** The board's calendar view as the server keeps it. */
  const stored = async (): Promise<Schemas['Block'] | undefined> => {
    const views = (await (
      await api.get(`/api/v1/boards/${board.boardId}/blocks`, { params: { type: 'view' } })
    ).json()) as Schemas['Block'][];
    return views.find((each) => each.fields.viewType === 'calendar');
  };

  await page.clock.setFixedTime(new Date('2026-11-10T00:00:00Z'));
  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${board.viewId}`);
  await signInOnPage(page, owner);

  // Made on the page: placed by the first date of the board, "Due".
  const header = page.getByTestId('view-header');
  await header.locator('[data-tool="add-view"]').click();
  await page.getByRole('menuitem', { name: 'Calendar', exact: true }).click();
  await expect.poll(async () => (await stored())?.title).toBe('Calendar view');
  const viewId = (await stored())?.id ?? '';
  await expect(page).toHaveURL(new RegExp(`/board/${board.boardId}/${viewId}$`));
  expect((await stored())?.fields.dateDisplayPropertyId).toBe('due');

  const calendar = page.getByTestId('calendar');
  await expect(calendar).toHaveAttribute('data-layout', 'bars');
  await expect(page.getByTestId('calendar-period')).toHaveText('November 2026');
  const grid = page.getByTestId('month-grid');
  await expect(grid.locator('[data-bar]').filter({ hasText: 'Pick apples' })).toHaveCount(1);
  // Thursday the 12th to Tuesday the 17th: two weeks, two bars.
  await expect(grid.locator('[data-bar]').filter({ hasText: 'Press cider' })).toHaveCount(2);

  const tool = header.locator('[data-tool="display-by"]');
  await expect(tool).toHaveAccessibleName('Display by: Due');
  await tool.click();
  await page.locator('[role="menuitem"][data-action="start"]').click();
  await expect.poll(async () => (await stored())?.fields.dateDisplayPropertyId).toBe('start');
  const week = (date: string) => grid.locator(`[data-week="${date}"] [data-bar]`);
  await expect(week('2026-11-01').filter({ hasText: 'Pick apples' })).toHaveCount(1);

  // After a reload the view is still a calendar, placed by "Start" as the server keeps it.
  await page.reload();
  await expect(calendar).toHaveAttribute('data-layout', 'bars');
  await expect(tool).toHaveAccessibleName('Display by: Start');
  await expect(week('2026-11-01').filter({ hasText: 'Pick apples' })).toHaveCount(1);
  await expect(week('2026-11-01').filter({ hasText: 'Press cider' })).toHaveCount(1);
  await expect(week('2026-11-08')).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/board/${board.boardId}/${viewId}$`));
  expect((await stored())?.fields.viewType).toBe('calendar');
  expect(errors).toEqual([]);
  await api.dispose();
});

test('a date changed on the calendar is kept by the server, whole, after a reload', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('calendar-move');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, { title: 'Moving calendar' });
  const made = await api.post(`/api/v1/boards/${board.boardId}/cards`, {
    data: { title: 'Press cider', properties: { due: { from: '2026-11-12', to: '2026-11-17' } } },
  });
  expect(made.ok()).toBe(true);
  const cardId = ((await made.json()) as Schemas['Card']).id;
  const due = async (): Promise<unknown> =>
    ((await (await api.get(`/api/v1/cards/${cardId}`)).json()) as Schemas['Card']).properties.due;

  await page.clock.setFixedTime(new Date('2026-11-10T00:00:00Z'));
  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${board.viewId}`);
  await signInOnPage(page, owner);
  await page.getByTestId('view-header').locator('[data-tool="add-view"]').click();
  await page.getByRole('menuitem', { name: 'Calendar', exact: true }).click();
  const grid = page.getByTestId('month-grid');
  await expect(grid.locator('[data-bar]').filter({ hasText: 'Press cider' })).toHaveCount(2);

  // From the cards of the 12th: the card's "…", "Change date", the 19th.
  await grid.locator('[data-day="2026-11-12"] [data-day-button]').click();
  const line = page.getByTestId('day-popover').locator('[data-card]');
  await line.hover();
  await line.getByRole('button', { name: 'Card menu: Press cider' }).click();
  await page.locator('[data-action="change-date"]').click();
  await page.getByTestId('day-picker').locator('[data-bits-day][data-value="2026-11-19"]').click();
  // The range keeps its six days.
  await expect.poll(due).toEqual({ from: '2026-11-19', to: '2026-11-24' });

  await page.reload();
  const week = (date: string) => grid.locator(`[data-week="${date}"] [data-bar]`);
  await expect(week('2026-11-15').filter({ hasText: 'Press cider' })).toHaveCount(1);
  await expect(week('2026-11-22').filter({ hasText: 'Press cider' })).toHaveCount(1);
  await expect(week('2026-11-08')).toHaveCount(0);
  expect(errors).toEqual([]);
  await api.dispose();
});
