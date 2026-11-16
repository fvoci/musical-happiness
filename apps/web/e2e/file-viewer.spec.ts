/**
 * The file viewer, opened from a card's attachments: a PDF turned through,
 * a Word document in Korean, a file it does not know and one too large, and
 * the frame the renderers run in kept off the network and the app's storage.
 *
 * With `FVOCI_SCREENS=1` the tests also take pictures of each state into
 * `target/viewer-p1/` (light and dark), at every size the project runs at
 * (the phone at 390 pixels).
 */
import { closeSync, ftruncateSync, openSync, writeSync } from 'node:fs';
import path from 'node:path';

import { expect, test, type FrameLocator, type Locator, type Page } from '@playwright/test';

import { DOCX_LINK, DOCX_TEXT, koreanDocx, koreanPdf } from '../tests/support/viewer-files.ts';

import { boardPath } from './address.ts';

const SCREENS = process.env.FVOCI_SCREENS === '1';
const SCREENS_DIR = path.resolve(import.meta.dirname, '../../../target/viewer-p1');

interface Dev {
  backend: {
    workspaces(): Promise<{ id: string }[]>;
    boards(workspaceId: string): Promise<{ id: string; title: string }[]>;
    views(boardId: string): Promise<{ id: string; title: string }[]>;
    cards(boardId: string): Promise<{ id: string; title: string }[]>;
  };
}

/** Opens the card everything is tried on, by its address. */
async function openCard(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  const place = await page.evaluate(async () => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const [workspace] = await backend.workspaces();
    const boards = await backend.boards(workspace?.id ?? '');
    const board = boards.find((each) => each.title.startsWith('Project Tasks'));
    if (!board) throw new Error('no board of tasks');
    const views = await backend.views(board.id);
    const card = (await backend.cards(board.id)).find(
      (each) => each.title === 'Identify dependencies',
    );
    if (!card) throw new Error('no card');
    const first = [...views].sort((a, b) => a.title.localeCompare(b.title))[0];
    return { boardId: board.id, viewId: first?.id ?? '', cardId: card.id };
  });
  await page.goto(boardPath(place.boardId, place.viewId, place.cardId));
  await expect(page.getByTestId('card-attachments')).toBeVisible();
}

async function attach(
  page: Page,
  name: string,
  buffer: Buffer,
  mimeType: string,
): Promise<Locator> {
  await page.getByTestId('attach-input').setInputFiles({ name, mimeType, buffer });
  const row = page.getByTestId('card-attachments').locator('[data-attachment]', { hasText: name });
  await expect(row).toBeVisible();
  return row;
}

const viewer = (page: Page): Locator => page.getByTestId('file-viewer');
const frameOf = (page: Page): FrameLocator =>
  page.frameLocator('[data-testid="file-viewer-frame"]');

async function picture(page: Page, name: string): Promise<void> {
  if (!SCREENS) return;
  const project = test.info().project.name;
  // The phone pictures at the narrowest width the app is drawn for.
  if (project === 'mobile') await page.setViewportSize({ width: 390, height: 844 });
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    // The app has turned (its root's class) and nothing on it is still fading.
    await page.waitForFunction(
      (dark) =>
        document.documentElement.classList.contains('dark') === dark &&
        document.getAnimations().every((animation) => animation.playState !== 'running'),
      theme === 'dark',
    );
    await page.screenshot({ path: path.join(SCREENS_DIR, `${name}-${project}-${theme}.png`) });
  }
  await page.emulateMedia({ colorScheme: null });
}

test('a PDF opens in the viewer, and its pages are turned with the buttons and the keys', async ({
  page,
}) => {
  // The viewer, its frame and the renderers are fetched when a file is first opened.
  const fetched: string[] = [];
  page.on('request', (request) => fetched.push(new URL(request.url()).pathname));
  const viewerFiles = (): string[] =>
    fetched.filter((file) => /\/(FileViewer|pdf|docx|viewer-frame)[-.][^/]*$/.test(file));
  await openCard(page);
  const row = await attach(page, '보고서.pdf', koreanPdf(3), 'application/pdf');
  expect(viewerFiles()).toEqual([]);
  await row.getByRole('link', { name: '보고서.pdf' }).click();

  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  expect(viewerFiles().some((file) => /\/pdf-[0-9a-f]{16}\.js$/.test(file))).toBe(true);
  expect(viewerFiles().some((file) => file.includes('/docx-'))).toBe(false);
  await expect(viewer(page).getByRole('heading', { name: '보고서.pdf' })).toBeVisible();
  const counter = page.getByTestId('file-viewer-page');
  await expect(counter).toContainText('1 / 3');
  await expect(counter).toContainText('Page 1 of 3');
  // The first page is drawn, and its text (Korean, through the bundled CMap) is there to select.
  const frame = frameOf(page);
  await expect(frame.locator('.page[data-page="1"] canvas')).toBeVisible();
  await expect(frame.locator('.page[data-page="1"] .textLayer')).toContainText('한국어 견본 1쪽');
  await picture(page, 'pdf');

  await viewer(page).getByRole('button', { name: 'Next page' }).click();
  await expect(counter).toContainText('2 / 3');
  await expect(frame.locator('.page[data-page="2"] canvas')).toBeVisible();

  // The keys work with the focus in the document as well as around it.
  await frame.locator('.page[data-page="2"]').click();
  await page.keyboard.press('ArrowRight');
  await expect(counter).toContainText('3 / 3');
  await expect(viewer(page).getByRole('button', { name: 'Next page' })).toBeDisabled();
  await page.keyboard.press('ArrowLeft');
  await expect(counter).toContainText('2 / 3');

  const zoom = page.getByTestId('file-viewer-zoom');
  await expect(zoom).toHaveText('100%');
  await page.keyboard.press('+');
  await expect(zoom).toHaveText('125%');
  await viewer(page).getByRole('button', { name: 'Zoom out' }).click();
  await viewer(page).getByRole('button', { name: 'Zoom out' }).click();
  await expect(zoom).toHaveText('75%');

  await page.keyboard.press('Escape');
  await expect(viewer(page)).toHaveCount(0);
  await expect(page.getByTestId('file-viewer-frame')).toHaveCount(0);
  // The card is still open, and the focus is back on the file's name.
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(row.getByRole('link', { name: '보고서.pdf' })).toBeFocused();
});

test('the frame reaches neither the network nor the app', async ({ page }) => {
  await openCard(page);
  await page.evaluate(() => {
    localStorage.setItem('fvoci:viewer-probe', 'secret');
  });
  const row = await attach(page, 'probe.pdf', koreanPdf(1), 'application/pdf');
  await row.getByRole('link', { name: 'probe.pdf' }).click();
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');

  const sandbox = await page.getByTestId('file-viewer-frame').getAttribute('sandbox');
  expect(sandbox).toBe('allow-scripts');
  const inside = page.frames().find((each) => each.url().endsWith('/static/viewer-frame.html'));
  if (!inside) throw new Error('no viewer frame');
  const found = await inside.evaluate(async () => {
    const attempt = async (action: () => unknown): Promise<string> => {
      try {
        const result = await action();
        return `reached: ${String(result)}`;
      } catch (error) {
        return `refused: ${error instanceof Error ? error.name : String(error)}`;
      }
    };
    return {
      origin: self.origin,
      // `location.origin` is "null" here; the address is still the app's.
      fetchApp: await attempt(async () => (await fetch(new URL('/', location.href))).status),
      fetchStatic: await attempt(async () => (await fetch(location.href)).status),
      storage: await attempt(() => localStorage.getItem('fvoci:viewer-probe')),
      cookies: await attempt(() => document.cookie),
      parent: await attempt(() => window.parent.document.title),
    };
  });
  // A message from another window than the frame (here, the page itself) is not taken.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        const done = (event: MessageEvent): void => {
          if (event.data !== 'probe-done') return;
          window.removeEventListener('message', done);
          resolve();
        };
        window.addEventListener('message', done);
        window.postMessage({ kind: 'failed', reason: 'locked' }, '*');
        window.postMessage('probe-done', '*');
      }),
  );
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  expect(found.origin).toBe('null');
  for (const [what, outcome] of Object.entries(found)) {
    if (what !== 'origin') expect(outcome, what).toMatch(/^refused/);
  }
});

test('a Word document in Korean shows its header, its table and its picture', async ({ page }) => {
  await openCard(page);
  const row = await attach(
    page,
    '회의록.docx',
    koreanDocx(),
    'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  );
  await row.getByRole('link', { name: '회의록.docx' }).click();
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  await expect(page.getByTestId('file-viewer-page')).toContainText('1 / 1');

  const frame = frameOf(page);
  const pageOne = frame.locator('section.docx').first();
  await expect(pageOne.locator('header')).toContainText(DOCX_TEXT.header);
  await expect(pageOne).toContainText(DOCX_TEXT.title);
  await expect(pageOne).toContainText(DOCX_TEXT.body);
  for (const cell of DOCX_TEXT.cells) await expect(pageOne.locator('table')).toContainText(cell);
  const image = pageOne.locator('img');
  await expect(image).toBeVisible();
  expect(await image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBe(200);
  // The Korean falls back to the app's own font rather than to whatever the system has.
  const family = await pageOne
    .getByText(DOCX_TEXT.title)
    .evaluate((element) => getComputedStyle(element).fontFamily);
  expect(family).toContain('Noto Sans KR');
  await expect(frame.locator('html')).toHaveAttribute('lang', 'en');
  await picture(page, 'docx');

  // A link of the document goes nowhere: the frame keeps the document, and the viewer its pages.
  const link = pageOne.getByRole('link', { name: DOCX_TEXT.link });
  await expect(link).toHaveAttribute('href', DOCX_LINK);
  await link.click();
  await link.click({ button: 'middle' });
  await expect(pageOne).toContainText(DOCX_TEXT.title);
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  const inside = page.frames().find((each) => each.url().endsWith('/static/viewer-frame.html'));
  expect(inside, 'the frame is still on its own document').toBeDefined();
});

test('a file of a kind the viewer does not draw is offered as a download', async ({ page }) => {
  await openCard(page);
  // Named as a PDF, but its bytes are not one.
  const row = await attach(page, 'notes.pdf', Buffer.from('just some notes'), 'application/pdf');
  await row.getByRole('link', { name: 'notes.pdf' }).click();
  await expect(viewer(page)).toHaveAttribute('data-status', 'unknown');
  const notice = page.getByTestId('file-viewer-notice');
  await expect(notice).toContainText("This file can't be previewed.");
  await expect(page.getByTestId('file-viewer-frame')).toHaveCount(0);
  await picture(page, 'unknown');

  const downloading = page.waitForEvent('download');
  await notice.getByRole('button', { name: 'Download' }).click();
  expect((await downloading).suggestedFilename()).toBe('notes.pdf');
});

test('a file over the limit is not read, and is offered as a download', async ({ page }, info) => {
  test.slow();
  await openCard(page);
  // A PDF's header, then nothing, up to one byte past the limit (a sparse file).
  const large = info.outputPath('large.pdf');
  const handle = openSync(large, 'w');
  writeSync(handle, '%PDF-1.7\n');
  ftruncateSync(handle, 50 * 1024 * 1024 + 1);
  closeSync(handle);
  await page.getByTestId('attach-input').setInputFiles(large);
  const row = page.getByTestId('card-attachments').locator('[data-attachment]', {
    hasText: 'large.pdf',
  });
  await row.getByRole('link', { name: 'large.pdf' }).click();
  await expect(viewer(page)).toHaveAttribute('data-status', 'too-large');
  await expect(page.getByTestId('file-viewer-notice')).toContainText('too large to preview');
  await expect(page.getByTestId('file-viewer-frame')).toHaveCount(0);
  await picture(page, 'too-large');
});
