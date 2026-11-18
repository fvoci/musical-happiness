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
  recordTicks,
  ticks,
  openCard,
  picture,
  timerLateness,
  viewer,
  viewerFrame,
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
  // Zoomed out, several slides are in view at once.
  for (const _ of [1, 2, 3]) await page.keyboard.press('-');
  await expect(zoom).toHaveText('50%');
  await expect(frame.locator('.slide[data-page="2"]')).toContainText(PPTX_TEXT.tableTitle);
  await picture(page, SCREENS, 'pptx-slides');
  await page.keyboard.press('Escape');
  await expect(viewer(page)).toHaveCount(0);
});

test('a deck at the narrowest width, in the dark: the slide fits and keeps its colours', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: 'dark' });
  await openCard(page);
  await open(page, '분기 보고.pptx', koreanPptx());
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  await expect(page.getByTestId('file-viewer-announce')).toHaveText('Slide 1 of 3');
  await expect(frameOf(page).locator('.slide[data-page="1"]')).toContainText(PPTX_TEXT.title);
  const seen = await viewerFrame(page).evaluate(() => {
    const box = document.querySelector('.slide');
    const drawn = box?.firstElementChild;
    return {
      dark: document.documentElement.classList.contains('dark'),
      scheme: getComputedStyle(document.documentElement).colorScheme,
      width: innerWidth,
      scrolls: (document.scrollingElement?.scrollWidth ?? 0) > innerWidth,
      box: box?.getBoundingClientRect().width ?? 0,
      background: drawn ? getComputedStyle(drawn).backgroundColor : '',
    };
  });
  // The frame is dark as the app is; the slide is as wide as the frame allows, keeps its own
  // white, and nothing scrolls sideways.
  expect(seen.dark).toBe(true);
  expect(seen.scheme).toBe('dark');
  expect(seen.scrolls).toBe(false);
  expect(seen.box).toBeGreaterThan(seen.width - 40);
  expect(seen.box).toBeLessThanOrEqual(seen.width - 32);
  expect(seen.background).toBe('rgb(255, 255, 255)');
  // The slides are turned and zoomed within the screen's width.
  for (const name of ['Previous slide', 'Next slide', 'Zoom out', 'Zoom in']) {
    const button = await viewer(page).getByRole('button', { name }).boundingBox();
    expect(button, name).not.toBeNull();
    expect((button?.x ?? -1) >= 0 && (button?.x ?? 0) + (button?.width ?? 0) <= 390, name).toBe(
      true,
    );
  }
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

/** The slides drawn, and those near the frame's view (where it draws them) not drawn yet. */
function drawing(page: Page): Promise<{ drawn: number; waiting: number }> {
  return viewerFrame(page).evaluate(() => {
    const boxes = [...document.querySelectorAll('.slide')];
    const near = (box: Element): boolean => {
      const { top, bottom } = box.getBoundingClientRect();
      return bottom > -innerHeight && top < 2 * innerHeight;
    };
    return {
      drawn: boxes.filter((box) => box.childElementCount > 0).length,
      waiting: boxes.filter((box) => box.childElementCount === 0 && near(box)).length,
    };
  });
}

/** Starts noting, in the frame, when each slide is put in its box (the times as `recordTicks`'). */
async function recordDrawn(page: Page): Promise<void> {
  await expect(page.getByTestId('file-viewer-frame')).toHaveCount(1);
  await expect
    .poll(() => page.frames().some((each) => each.url().endsWith('/static/viewer-frame.html')))
    .toBe(true);
  await viewerFrame(page).evaluate(() => {
    const noted = window as unknown as { fvociDrawn: number[] };
    noted.fvociDrawn = [];
    new MutationObserver((records) => {
      for (const record of records) {
        const box = record.target;
        if (
          box instanceof HTMLElement &&
          box.classList.contains('slide') &&
          record.addedNodes.length > 0
        ) {
          noted.fvociDrawn.push(performance.timeOrigin + performance.now());
        }
      }
    }).observe(document.body, { childList: true, subtree: true });
  });
}

/** Small slides of many text boxes: many in view, each a while to draw. */
const boxes = (): Buffer => heavyPptx(40, 80, { cx: 3_048_000, cy: 914_400 });

test('while slides are drawn, the app answers', async ({ page }) => {
  await openCard(page);
  await recordTicks(page);
  await open(page, 'boxes.pptx', boxes());
  await recordDrawn(page);
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  const drawnTimes = (): Promise<number[]> =>
    viewerFrame(page).evaluate(() => (window as unknown as { fvociDrawn: number[] }).fvociDrawn);
  await expect.poll(async () => (await drawnTimes()).length).toBeGreaterThanOrEqual(5);
  // Slide after slide was put in place, and between each and the next the page's timer ran:
  // the thread was given back between them, however long each took.
  const drawn = await drawnTimes();
  const ran = await ticks(page);
  const held = drawn
    .slice(1)
    .map((time, index) => [drawn[index] ?? 0, time] as const)
    .filter(([from, to]) => !ran.some((tick) => tick > from && tick < to));
  expect(held).toEqual([]);
});

test('while slides are drawn, "Close" closes the viewer', async ({ page }) => {
  await openCard(page);
  await open(page, 'boxes.pptx', boxes());
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
  expect((await drawing(page)).waiting).toBeGreaterThan(0);
  await page.getByTestId('file-viewer-close').click();
  await expect(viewer(page)).toHaveCount(0, { timeout: 2000 });
});
