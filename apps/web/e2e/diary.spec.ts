/**
 * A diary, end to end, on the in-memory backend: the reader's own space has
 * one way in (made as it is pressed), the shared workspace none; its page
 * shows one day of the year in every year, this year's open and the years
 * before it folded; an entry is made by what is written first, not before;
 * a year nobody wrote on is written on afterwards; the days step through
 * `02-29`; midnight moves today and leaves what is being written where it
 * is; and all of it goes by the keyboard alone and fits a phone.
 *
 * The world in memory starts anew on a load: going about inside the page is
 * done as the app's links do (`goInside`). The same diary over a load and
 * from two devices is tested against the server (`e2e-live/diary.spec.ts`).
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import { screensDir } from './screens.ts';
import { openApp, showSidebar, sidebar } from './shell.support.ts';
import { goInside } from './wiki.support.ts';

test.use({ timezoneId: 'Asia/Seoul', locale: 'en-US' });

/** A Saturday morning in Seoul. */
const MORNING = new Date('2026-11-21T10:00:00+09:00');
const ID = '[0-9a-f-]{36}';
const ON_A_DAY = new RegExp(`/w/${ID}/diaries/(${ID})/(\\d{2}-\\d{2})$`, 'u');

const diaryPage = (page: Page): Locator => page.getByTestId('diary-page');
const year = (page: Page, date: string): Locator =>
  diaryPage(page).locator(`[data-testid="diary-year"][data-date="${date}"]`);
const editors = (page: Page): Locator =>
  diaryPage(page).locator('[data-testid="diary-year"] [contenteditable="true"]');
const editorOf = (cell: Locator): Locator => cell.locator('[contenteditable="true"]').first();

function dayOf(page: Page): { diaryId: string; monthDay: string } {
  const [, diaryId = '', monthDay = ''] = ON_A_DAY.exec(page.url()) ?? [];
  return { diaryId, monthDay };
}

/** How many entries the diary shown has on a day of the year, as the backend has them. */
async function entriesOn(page: Page, monthDay: string): Promise<number> {
  return page.evaluate(
    async ([diaryId, day]) => {
      const dev = (
        window as unknown as {
          fvociDev: {
            backend: { diary: { entriesOn(id: string, md: string): Promise<unknown[]> } };
          };
        }
      ).fvociDev;
      return (await dev.backend.diary.entriesOn(diaryId, day)).length;
    },
    [dayOf(page).diaryId, monthDay] as const,
  );
}

async function switchTo(page: Page, name: string): Promise<void> {
  await showSidebar(page);
  await sidebar(page).getByTestId('workspace-switcher').click();
  await page
    .getByRole('menu', { name: 'Switch workspace', exact: true })
    .getByRole('menuitem', { name, exact: true })
    .click();
}

/** The app at `time`, in the own space, its first diary made and open on today. */
async function openDiary(page: Page, time = MORNING): Promise<void> {
  await page.clock.install({ time });
  await openApp(page);
  await switchTo(page, 'My space');
  await showSidebar(page);
  await sidebar(page).getByTestId('open-diary').click();
  await expect(page).toHaveURL(ON_A_DAY);
  await expect(diaryPage(page)).toBeVisible();
}

/** Goes to another day of the year of the diary shown, as a link would. */
async function goToDay(page: Page, monthDay: string): Promise<void> {
  await goInside(page, new URL(page.url()).pathname.replace(/\d{2}-\d{2}$/u, monthDay));
  await expect(diaryPage(page)).toHaveAttribute('data-month-day', monthDay);
}

test('the shared workspace has no diary; the own space has one way in, made as it is pressed', async ({
  page,
}) => {
  await openApp(page);
  await showSidebar(page);
  await expect(sidebar(page).getByTestId('open-diary')).toHaveCount(0);
  await sidebar(page).getByTestId('add-board').click();
  await expect(page.getByTestId('template-picker')).toBeVisible();
  await expect(page.getByTestId('template-picker')).not.toContainText(/diary/iu);
  await page.keyboard.press('Escape');

  await switchTo(page, 'My space');
  await showSidebar(page);
  const way = sidebar(page).getByTestId('open-diary');
  await expect(way).toHaveText('Diary');
  await way.click();
  await expect(page).toHaveURL(ON_A_DAY);
  await showSidebar(page);
  // Now it is in the list beside the boards and wikis, with its own mark, and the way in is gone.
  const link = sidebar(page).getByRole('link', { name: 'Diary, diary', exact: true });
  await expect(link).toBeVisible();
  await expect(link.locator('[data-board-icon]')).toHaveAttribute('data-kind', 'diary');
  await expect(way).toHaveCount(0);
  await expect(link).toHaveAttribute('aria-current', 'page');
});

test('today’s page: the day at the head, this year open, an entry made by what is written first', async ({
  page,
}) => {
  await openDiary(page);
  expect(dayOf(page).monthDay).toBe('11-21');
  await expect(page.getByTestId('diary-day')).toHaveText('November 21');
  await expect(page.getByTestId('diary-today')).toHaveAttribute('aria-current', 'date');
  const today = year(page, '2026-11-21');
  await expect(today).toHaveAttribute('data-open', '');
  await expect(today).not.toHaveAttribute('data-written');
  // Nothing is made for a day before the reader writes.
  expect(await entriesOn(page, '11-21')).toBe(0);

  await editorOf(today).click();
  await page.keyboard.type('Rain all day');
  await expect(today).toHaveAttribute('data-written', '');
  await expect(today.getByTestId('entry-body')).toHaveAttribute('data-editor', 'ready');
  await page.keyboard.type(', then sun.');
  await expect(editorOf(today)).toHaveText('Rain all day, then sun.');
  await expect(editors(page)).toHaveCount(1);
  expect(await entriesOn(page, '11-21')).toBe(1);

  // Away and back: what was written is there.
  await page.getByTestId('diary-next').click();
  await expect(page).toHaveURL(/\/11-22$/u);
  await expect(page.getByTestId('diary-today')).not.toHaveAttribute('aria-current');
  await expect(year(page, '2026-11-22').getByTestId('entry-to-come')).toHaveText('Not here yet');
  await page.goBack();
  await expect(page).toHaveURL(/\/11-21$/u);
  await expect(year(page, '2026-11-21')).toContainText('Rain all day, then sun.');
});

test('a year before: written afterwards, one open at a time, the others folded', async ({
  page,
}) => {
  await openDiary(page);
  const today = year(page, '2026-11-21');
  await editorOf(today).click();
  await page.keyboard.type('This year.');
  await expect(today).toHaveAttribute('data-written', '');
  // The room writes the body a while after the last key, while it is open.
  await page.clock.runFor(3000);

  await page.getByTestId('diary-earlier').click();
  const last = year(page, '2025-11-21');
  await expect(last.getByTestId('entry-blank')).toHaveText('Not written');
  await last.getByTestId('entry-blank').click();
  await expect(last).toHaveAttribute('data-open', '');
  await expect(editors(page)).toHaveCount(1);
  // The caret is in the day opened.
  await page.keyboard.type('A year ago.');
  await expect(last).toHaveAttribute('data-written', '');
  // This year's is folded: its first lines, read only.
  await expect(today.getByTestId('entry-preview')).toContainText('This year.');

  await today.getByTestId('entry-open').click();
  await expect(today).toHaveAttribute('data-open', '');
  await expect(last.getByTestId('entry-preview')).toContainText('A year ago.');
  await expect(editors(page)).toHaveCount(1);
  // Years after the diary was made, and before, in that order.
  await expect(diaryPage(page).getByTestId('diary-year')).toHaveCount(2);
  const dates = await diaryPage(page)
    .getByTestId('diary-year')
    .evaluateAll((cells) => cells.map((cell) => cell.getAttribute('data-date')));
  expect(dates).toEqual(['2026-11-21', '2025-11-21']);
});

test('an emptied entry is a day not written again', async ({ page }) => {
  await openDiary(page);
  const today = year(page, '2026-11-21');
  await editorOf(today).click();
  await page.keyboard.type('Oops');
  await expect(today.getByTestId('entry-body')).toHaveAttribute('data-editor', 'ready');
  for (let left = 4; left > 0; left -= 1) await page.keyboard.press('Backspace');
  await expect(editorOf(today)).toHaveText('');
  await page.getByTestId('diary-next').click();
  await page.getByTestId('diary-previous').click();
  await expect(page).toHaveURL(/\/11-21$/u);
  await expect(year(page, '2026-11-21')).not.toHaveAttribute('data-written');
});

test('the days step through 02-29, which holds the leap years alone', async ({ page }) => {
  await openDiary(page);
  await goToDay(page, '02-28');
  await expect(page.getByTestId('diary-day')).toHaveText('February 28');
  await page.getByTestId('diary-next').click();
  await expect(page).toHaveURL(/\/02-29$/u);
  await expect(page.getByTestId('diary-day')).toHaveText('February 29');
  await expect(diaryPage(page)).toContainText('February 29 comes in leap years only.');
  // 2026 has none: the way to the last leap year is there all the same.
  await expect(diaryPage(page).getByTestId('diary-year')).toHaveCount(0);
  await expect(page.getByTestId('diary-earlier')).toHaveText('Show 2024');
  await page.getByTestId('diary-earlier').click();
  await expect(year(page, '2024-02-29').getByTestId('entry-blank')).toBeVisible();
  await page.getByTestId('diary-next').click();
  await expect(page).toHaveURL(/\/03-01$/u);
  await page.getByTestId('diary-previous').click();
  await expect(page).toHaveURL(/\/02-29$/u);
  // A day no year has is not found.
  await goInside(page, new URL(page.url()).pathname.replace(/02-29$/u, '02-30'));
  await expect(page.getByTestId('not-found')).toBeVisible();
});

test('midnight moves today; what is being written stays on its day', async ({ page }) => {
  await openDiary(page, new Date('2026-11-21T23:58:00+09:00'));
  const late = year(page, '2026-11-21');
  await editorOf(late).click();
  await page.keyboard.type('Late');
  await expect(late).toHaveAttribute('data-written', '');
  await page.clock.fastForward('05:00');
  await expect(page.getByTestId('diary-today')).not.toHaveAttribute('aria-current');
  await expect(page).toHaveURL(/\/11-21$/u);
  await expect(late).toHaveAttribute('data-open', '');
  await page.keyboard.type(' night.');
  await expect(editorOf(late)).toHaveText('Late night.');

  await page.getByTestId('diary-today').click();
  await expect(page).toHaveURL(/\/11-22$/u);
  await expect(page.getByTestId('diary-today')).toHaveAttribute('aria-current', 'date');
  await expect(year(page, '2026-11-22')).toHaveAttribute('data-open', '');
});

test('by the keyboard alone: from day to day, from year to year, a year opened', async ({
  page,
}) => {
  await openDiary(page);
  await page.getByTestId('diary-earlier').click();
  await page.getByTestId('diary-previous').focus();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/11-20$/u);
  await page.keyboard.press('Tab');
  await expect(page.getByTestId('diary-today')).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/11-21$/u);
  // From the head down to the years.
  const blank = year(page, '2025-11-21').getByTestId('entry-blank');
  await page.getByTestId('diary-pick').focus();
  const focused = (): Promise<boolean> => blank.evaluate((it) => it === document.activeElement);
  for (let step = 0; step < 6 && !(await focused()); step += 1) await page.keyboard.press('Tab');
  await expect(blank).toBeFocused();
  await expect(blank).toHaveAccessibleName('Write the entry of Friday, November 21, 2025');
  await page.keyboard.press('Enter');
  await page.keyboard.type('Typed by keys.');
  await expect(year(page, '2025-11-21')).toHaveAttribute('data-written', '');
});

test('the menu renames and deletes the diary, and the way in comes back', async ({ page }) => {
  await openDiary(page);
  await page.getByTestId('diary-menu').click();
  await page.locator('[data-action="rename"]').click();
  const naming = page.getByTestId('diary-name-dialog');
  await naming.getByRole('textbox').fill('Days');
  await naming.getByRole('textbox').press('Enter');
  await expect(page.getByTestId('diary-title')).toHaveText('Days');
  await showSidebar(page);
  await expect(sidebar(page).getByRole('link', { name: 'Days, diary', exact: true })).toBeVisible();

  await page.keyboard.press('Escape');
  await page.getByTestId('diary-menu').click();
  await page.locator('[data-action="delete"]').click();
  await page
    .getByTestId('confirm-dialog')
    .getByRole('button', { name: 'Delete', exact: true })
    .click();
  await expect(diaryPage(page)).toHaveCount(0);
  await showSidebar(page);
  await expect(sidebar(page).getByTestId('open-diary')).toBeVisible();
});

test('fits a phone, light and dark', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDiary(page);
  await page.keyboard.press('Escape');
  await page.getByTestId('diary-earlier').click();
  await editorOf(year(page, '2026-11-21')).click();
  await page.keyboard.type('Small screen.');
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    const wide = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(wide).toBeLessThanOrEqual(390);
    for (const id of ['diary-previous', 'diary-today', 'diary-next', 'diary-pick', 'diary-menu']) {
      await expect(page.getByTestId(id)).toBeInViewport();
    }
    await page.screenshot({ path: `${screensDir('diary')}/phone-${scheme}.png` });
  }
});
