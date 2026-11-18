/**
 * PowerPoint decks in the file viewer, opened from a card's attachments: a
 * deck in Korean turned through slide by slide, its links kept in the frame,
 * decks that pass a ceiling of the PowerPoint worker and an old PowerPoint
 * file offered as a download, and the app kept answering (and "Close"
 * closing) while a deck is read and while its slides are drawn.
 *
 * With `FVOCI_SCREENS=1` the tests take pictures into `target/viewer-p3/`.
 */
import { expect, test, type Page } from '@playwright/test';

import { HOSTILE_PPTX, heavyPptx, slowPptx } from '../tests/support/hostile-pptx.ts';
import { koreanPptx, PPTX_TEXT } from '../tests/support/viewer-pptx.ts';

import {
  attach,
  frameOf,
  frameReaches,
  openCard,
  picture,
  timerLateness,
  viewer,
} from './file-viewer.support.ts';

const PPTX_TYPE = 'application/vnd.openxmlformats-officedocument.presentationml.presentation';
const SCREENS = 'viewer-p3';

async function open(page: Page, name: string, file: Buffer): Promise<void> {
  const row = await attach(page, name, file, PPTX_TYPE);
  await row.getByRole('link', { name }).click();
}

test('a deck in Korean opens in the viewer, and its slides are turned', async ({ page }) => {
  const fetched: string[] = [];
  page.on('request', (request) => fetched.push(new URL(request.url()).pathname));
  await openCard(page);
  await open(page, '분기 보고.pptx', koreanPptx());
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  // Only the PowerPoint renderer is fetched, the Word and PDF renderers not.
  expect(fetched.some((file) => /\/pptx-[0-9a-f]{16}\.js$/.test(file))).toBe(true);
  expect(fetched.some((file) => /\/(docx|pdf)-[0-9a-f]{16}\.js$/.test(file))).toBe(false);
  const counter = page.getByTestId('file-viewer-page');
  await expect(counter).toContainText('1 / 3');
  await expect(counter).toContainText('Slide 1 of 3');

  const frame = frameOf(page);
  const first = frame.locator('.slide[data-page="1"]');
  await expect(first).toContainText(PPTX_TEXT.title);
  await expect(first).toContainText(PPTX_TEXT.body);
  const image = first.locator('img');
  await expect(image).toBeVisible();
  expect(await image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBe(300);
  // The Korean falls back to the app's own font rather than to whatever the system has.
  const family = await first
    .getByText(PPTX_TEXT.title)
    .evaluate((element) => getComputedStyle(element).fontFamily);
  expect(family).toContain('Noto Sans KR');
  await picture(page, SCREENS, 'pptx');

  await viewer(page).getByRole('button', { name: 'Next slide' }).click();
  await expect(counter).toContainText('2 / 3');
  const second = frame.locator('.slide[data-page="2"]');
  await expect(second).toBeInViewport();
  for (const cell of PPTX_TEXT.cells.flat()) await expect(second).toContainText(cell);
  // Charts are not drawn: a box says so.
  await expect(second.locator('[data-fvoci-chart]')).toHaveText(
    'Charts are not shown in the preview',
  );
  // A link to a web page is text that goes nowhere.
  const link = second.getByText(PPTX_TEXT.link);
  await expect(link).toBeVisible();
  expect(await link.evaluate((element) => element.closest('[href]'))).toBeNull();
  await link.click();
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');

  // The keys turn the slides with the focus in the deck as well as around it.
  await second.click();
  await page.keyboard.press('ArrowRight');
  await expect(counter).toContainText('3 / 3');
  await expect(frame.locator('.slide[data-page="3"]')).toContainText(PPTX_TEXT.third);
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('ArrowLeft');
  await expect(counter).toContainText('1 / 3');
  // A jump to another slide, which the deck has as a link, goes to that slide.
  await first.getByText(PPTX_TEXT.jump).click();
  await expect(counter).toContainText('3 / 3');

  const zoom = page.getByTestId('file-viewer-zoom');
  await page.keyboard.press('+');
  await expect(zoom).toHaveText('125%');
  await page.keyboard.press('Escape');
  await expect(viewer(page)).toHaveCount(0);
});

test("a deck's frame reaches neither the network nor the app", async ({ page }) => {
  await openCard(page);
  await page.evaluate(() => {
    localStorage.setItem('fvoci:viewer-probe', 'secret');
  });
  await open(page, 'probe.pptx', koreanPptx());
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  const found = await frameReaches(page);
  expect(found.origin).toBe('null');
  for (const [what, outcome] of Object.entries(found)) {
    if (what !== 'origin') expect(outcome, what).toMatch(/^refused/);
  }
});

for (const ceiling of ['slides', 'textBodies', 'pixels'] as const) {
  test(`a deck past a ceiling is not drawn, and is offered as a download: ${ceiling}`, async ({
    page,
  }) => {
    await openCard(page);
    await open(page, `${ceiling}.pptx`, HOSTILE_PPTX[ceiling]?.() ?? Buffer.alloc(0));
    await expect(viewer(page)).toHaveAttribute('data-status', 'failed');
    const notice = page.getByTestId('file-viewer-notice');
    await expect(notice).toContainText("This file's contents are too large to preview.");
    await expect(page.getByTestId('file-viewer-frame')).toHaveCount(0);
    if (ceiling === 'slides') await picture(page, SCREENS, 'pptx-too-large');
    const downloading = page.waitForEvent('download');
    await notice.getByRole('button', { name: 'Download' }).click();
    expect((await downloading).suggestedFilename()).toBe(`${ceiling}.pptx`);
  });
}

test('an old PowerPoint file (not a ZIP archive) is offered as a download', async ({ page }) => {
  await openCard(page);
  const compound = Buffer.alloc(2048);
  compound.set([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
  const row = await attach(page, '옛 발표.ppt', compound, 'application/vnd.ms-powerpoint');
  await row.getByRole('link', { name: '옛 발표.ppt' }).click();
  await expect(viewer(page)).toHaveAttribute('data-status', 'unknown');
  await expect(page.getByTestId('file-viewer-notice')).toContainText(
    "This file can't be previewed.",
  );
});

test('while a deck is read, the app answers and "Close" closes the viewer', async ({ page }) => {
  await openCard(page);
  // Megabytes of XML under every ceiling: the worker reads it for seconds.
  await open(page, 'long.pptx', slowPptx(900, 3_000));
  await expect(viewer(page)).toHaveAttribute('data-status', 'opening');
  expect(await timerLateness(page)).toBeLessThan(500);
  await expect(viewer(page)).toHaveAttribute('data-status', 'opening');
  await page.getByTestId('file-viewer-close').click();
  await expect(viewer(page)).toHaveCount(0, { timeout: 2000 });
});

test('while slides are drawn, the app answers and "Close" closes the viewer', async ({ page }) => {
  await openCard(page);
  // Slides of many text boxes, each a while to draw; zoomed out, many are in view at once.
  const slides = 30;
  await open(page, 'boxes.pptx', heavyPptx(slides, 120));
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  await viewer(page).getByRole('button', { name: 'Zoom out' }).click();
  await viewer(page).getByRole('button', { name: 'Zoom out' }).click();
  const frame = frameOf(page);
  const drawn = (): Promise<number> => frame.locator('.slide:has(> *)').count();
  await expect.poll(drawn).toBeGreaterThan(1);
  // The slides come one by one, and between them the page's timers keep their time.
  for (let check = 0; check < 3; check += 1) {
    expect(await timerLateness(page)).toBeLessThan(500);
  }
  expect(await drawn()).toBeLessThan(slides);
  await page.getByTestId('file-viewer-close').click();
  await expect(viewer(page)).toHaveCount(0, { timeout: 2000 });
});
