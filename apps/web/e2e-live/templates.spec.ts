/**
 * Templates on the real server: a board made from a built-in template, a
 * template made from a board and changed on its own page, the workspace's
 * template offered to be changed again from "Add board", and the welcome
 * board with its views, its pictures and its document, in either language.
 */
import path from 'node:path';

import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

import { screensDir } from '../e2e/screens.ts';

import { boardLink, boardMenu, chooseFromMenu, sidebar } from '../e2e/shell.support.ts';

import {
  boardPath,
  makeBoard,
  person,
  placeOf,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

async function templates(api: APIRequestContext, workspaceId: string) {
  const answer = await api.get(`/api/v1/w/${workspaceId}/templates`);
  expect(answer.ok()).toBe(true);
  return (await answer.json()) as Schemas['Container'][];
}

async function views(api: APIRequestContext, workspaceId: string, boardId: string) {
  return (await (
    await api.get(`/api/v1/w/${workspaceId}/boards/${boardId}/blocks`, { params: { type: 'view' } })
  ).json()) as Schemas['Block'][];
}

test('a board is made from a template, and a template from a board is changed on its page', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('templater');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const mine = await makeBoard(api, { title: 'Weekly plan', cards: [{ title: 'Water' }] });
  const builtIn = (await templates(api, mine.workspaceId)).find(
    (template) => template.title === 'Issue tracker',
  );
  expect(builtIn?.isBuiltin).toBe(true);

  const errors = watchErrors(page);
  await page.goto(boardPath(mine, mine.viewId));
  await signInOnPage(page, owner);

  // "Add board" offers an empty board and the templates; a built-in one is used.
  await sidebar(page).getByTestId('add-board').click();
  const picker = page.getByTestId('template-picker');
  await expect(picker.getByTestId('empty-board')).toBeVisible();
  const issueTracker = picker.locator('[data-template="Issue tracker"]');
  // A built-in template is nobody's to change.
  await expect(issueTracker.getByRole('button', { name: /^Change the template/ })).toHaveCount(0);
  await issueTracker.getByRole('button', { name: 'Use this template: Issue tracker' }).click();
  await expect(boardLink(page, 'Issue tracker')).toHaveAttribute('aria-current', 'page');
  const made = placeOf(page.url()).boardId;
  const copy = (await (
    await api.get(`/api/v1/w/${mine.workspaceId}/containers/${made}`)
  ).json()) as Schemas['Container'];
  // In the reader's workspace (sent as `toWorkspace`: a built-in template is kept apart from every workspace).
  expect(copy).toMatchObject({
    title: 'Issue tracker',
    isTemplate: false,
    type: 'P',
    workspaceId: mine.workspaceId,
  });
  expect((await views(api, mine.workspaceId, made)).length).toBe(
    (await views(api, mine.workspaceId, builtIn?.id ?? '')).length,
  );

  // A template from a board: it opens on its page, and is changed there.
  await chooseFromMenu(page, boardMenu(page, 'Weekly plan'), 'template-from-board');
  await expect(page.getByTestId('template-note')).toBeVisible();
  const title = page.getByTestId('top-bar').getByTestId('board-title');
  await expect(title).toHaveValue('New board template');
  await title.fill('Our weekly plan');
  await title.press('Enter');
  await expect
    .poll(async () =>
      (await templates(api, mine.workspaceId))
        .filter((one) => !one.isBuiltin)
        .map((one) => one.title),
    )
    .toEqual(['Our weekly plan']);
  // No board of the sidebar.
  await expect(boardLink(page, 'Our weekly plan')).toHaveCount(0);
  const templateId = placeOf(page.url()).boardId;

  // Offered again from "Add board", to be changed or used.
  await page.goto(boardPath(mine, mine.viewId));
  await sidebar(page).getByTestId('add-board').click();
  await picker.getByRole('button', { name: 'Change the template Our weekly plan' }).click();
  await expect(page).toHaveURL(new RegExp(boardPath({ ...mine, boardId: templateId })));
  await expect(page.getByTestId('template-note')).toBeVisible();

  await sidebar(page).getByTestId('add-board').click();
  await picker.getByRole('button', { name: 'Use this template: Our weekly plan' }).click();
  await expect(boardLink(page, 'Our weekly plan')).toHaveAttribute('aria-current', 'page');
  await expect(page.getByText('Water')).toBeVisible();
  await expect(page.locator('[data-problem]')).toHaveCount(0);

  await api.dispose();
  expect(errors).toEqual([]);
});

/** Pictures the page as wide as it is and 390 pixels wide, with `FVOCI_SCREENS=1`: `target/screens/templates/`. */
async function shot(page: Page, name: string): Promise<void> {
  if (process.env.FVOCI_SCREENS !== '1') return;
  const folder = screensDir('templates');
  const wide = page.viewportSize() ?? { width: 1280, height: 720 };
  await page.screenshot({ path: path.join(folder, `${name}-wide.png`) });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: path.join(folder, `${name}-390.png`) });
  await page.setViewportSize(wide);
}

/** Makes the welcome board for whoever `api` is signed in as, on a screen in `language`, and answers where it is. */
async function welcomeBoard(
  api: APIRequestContext,
  workspaceId: string,
  language: string,
): Promise<{ workspaceId: string; boardId: string }> {
  const answer = await api.post(`/api/v1/w/${workspaceId}/onboard`, { params: { language } });
  expect(answer.ok()).toBe(true);
  const { boardId } = (await answer.json()) as Schemas['WelcomeBoard'];
  return { workspaceId, boardId };
}

test('the welcome board shows its five views, its pictures and its document, in English and in Korean', async ({
  page,
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('welcomed');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const { workspaceId } = await makeBoard(api, { title: 'Before the welcome' });
  const english = await welcomeBoard(api, workspaceId, 'en');

  const errors = watchErrors(page);
  await page.goto(boardPath(english));
  await signInOnPage(page, owner);
  await expect(boardLink(page, 'Getting started with fvoci')).toHaveAttribute(
    'aria-current',
    'page',
  );

  // Five views, each of its own kind. The tabs are in the order of their titles, and
  // the board is the first: what the welcome board opens on.
  const tabs = page.getByTestId('view-tabs');
  await expect(tabs.getByRole('link')).toHaveText([
    'Board',
    'Calendar',
    'Gallery',
    'Gantt',
    'Table',
  ]);
  await expect(page.getByTestId('kanban')).toBeVisible();
  await expect(page.getByText('Open a card', { exact: true })).toBeVisible();
  await shot(page, 'welcome-en-board');
  for (const [view, shown] of [
    ['Calendar', 'calendar'],
    ['Gallery', 'gallery'],
    ['Gantt', 'gantt'],
    ['Table', 'table-view'],
  ] as const) {
    await tabs.getByRole('link', { name: view, exact: true }).click();
    await expect(page.getByTestId(shown)).toBeVisible();
    if (view === 'Gallery') {
      // The first picture of a body is the face of its card: a file of this board's own.
      const picture = page.getByTestId('gallery').locator('img').first();
      await expect
        .poll(() => picture.evaluate((image: HTMLImageElement) => image.naturalWidth))
        .toBeGreaterThan(0);
    }
    await shot(page, `welcome-en-${view.toLowerCase()}`);
  }

  // The document of a card opens in the viewer, where it is.
  await tabs.getByRole('link', { name: 'Board', exact: true }).click();
  await page.getByText('Attach files and open them right here', { exact: true }).click();
  const block = page.locator('[data-file-block]');
  await expect(block).toHaveAttribute('data-state', 'shown');
  await shot(page, 'welcome-en-card');
  await block.getByRole('button', { name: 'View Try the viewer.docx' }).click();
  const viewer = page.getByTestId('file-viewer');
  await expect(viewer).toHaveAttribute('data-status', 'shown');
  await expect(
    page.frameLocator('[data-testid="file-viewer-frame"]').locator('section.docx'),
  ).toContainText('What you are looking at');
  await shot(page, 'welcome-en-viewer');
  await page.getByTestId('file-viewer-close').click();
  await expect(viewer).toHaveCount(0);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  expect(errors).toEqual([]);

  // The same account on a Korean screen: the board is written in Korean.
  const korean = await welcomeBoard(api, workspaceId, 'ko');
  const context = await browser.newContext({
    locale: 'ko-KR',
    baseURL: base,
    storageState: await page.context().storageState(),
  });
  const koreanPage = await context.newPage();
  const koreanErrors = watchErrors(koreanPage);
  await koreanPage.goto(boardPath(korean));
  await expect(boardLink(koreanPage, 'fvoci 시작하기')).toHaveAttribute('aria-current', 'page');
  await expect(koreanPage.getByTestId('view-tabs').getByRole('link')).toHaveText([
    '가이드',
    '간트',
    '갤러리',
    '달력',
    '표',
  ]);
  await expect(koreanPage.getByText('카드를 열어 보세요', { exact: true })).toBeVisible();
  await shot(koreanPage, 'welcome-ko-board');
  await koreanPage.getByText('파일을 올리고 그 자리에서 여세요', { exact: true }).click();
  await expect(koreanPage.locator('[data-file-block]')).toHaveAttribute('data-state', 'shown');
  await shot(koreanPage, 'welcome-ko-card');
  expect(koreanErrors).toEqual([]);
  await context.close();
  await api.dispose();
});
