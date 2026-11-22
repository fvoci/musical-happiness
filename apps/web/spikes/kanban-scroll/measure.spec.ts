/**
 * How a kanban column of some 1,600 cards scrolls on a phone's screen. Not a
 * test: it prints numbers (`[kanban-scroll] …`), and runs only when asked for.
 *
 * What it measures, on the app itself, built, on its in-memory backend:
 * - how many elements the column holds, and how high its cards are;
 * - how long the view takes to be drawn;
 * - the time between frames while the column is scrolled slowly (8 pixels a
 *   frame), flung down (80) and flung back up from its end, and in Chromium the
 *   long tasks and the time spent laying out, styling and running scripts;
 * - whether the cards in sight jump while the column is flung up from its end
 *   over cards never drawn before (a card's height guessed wrong);
 * - the time between frames while a mouse drags a card down the column and
 *   holds it at the bottom edge, where the column scrolls by itself.
 *
 * A preview of `dist/` must be running, at the address given:
 *
 *   pnpm --filter web run build && node tools/preview.ts 20542 &
 *   FVOCI_SPIKE_APP=http://127.0.0.1:20542 pnpm --filter web run spikes:e2e kanban-scroll \
 *     --project phone-chromium
 *
 * `phone-webkit` takes WebKit from a Playwright server (`PW_TEST_CONNECT_WS_ENDPOINT`);
 * `phone-chromium` slows the CPU four times, as a phone's is to a desktop's.
 *
 * - `FVOCI_SPIKE_CARDS=100`: a shorter column, for what a frame costs whatever
 *   the column holds.
 * - `FVOCI_SPIKE_DRAW_ALL=1`: every card drawn whether in sight or not
 *   (`content-visibility: visible`), so that the two are measured on one build.
 * - `FVOCI_SPIKE_CSS`: rules laid over the app's, to try out another way of
 *   drawing the column.
 * - `FVOCI_SPIKE_FRAMES=1`: every frame's time of the scrolled runs, besides
 *   their summary.
 * - `FVOCI_SPIKE_SCALE=1`: a pixel to a point instead of the phone's three.
 */
import { loadavg } from 'node:os';

import { expect, test, type CDPSession, type Page } from '@playwright/test';

import { cardsBox, fitPane, paneOf } from '../../e2e/kanban.support.ts';
import { LONG, LONG_COLUMN, makeLongColumn, showView } from '../../e2e/kanban-long.support.ts';

const APP = process.env.FVOCI_SPIKE_APP;
const DRAW_ALL = process.env.FVOCI_SPIKE_DRAW_ALL === '1';
// Rules laid over the app's for one run, to try out another way of drawing the column.
const CSS_RULES = process.env.FVOCI_SPIKE_CSS ?? '';
// Every frame's time of the scrolled runs, besides their summary.
const FRAMES = process.env.FVOCI_SPIKE_FRAMES === '1';
// How many cards the column gets: fewer, to see what a frame costs whatever the column holds.
const COUNT = Number(process.env.FVOCI_SPIKE_CARDS ?? LONG);
// Pixels to a point: 1 instead of the phone's 3, to see what is left once painting costs less.
const SCALE = process.env.FVOCI_SPIKE_SCALE;

// A trace copies the whole document at every step, which is what would be measured.
test.use({ trace: 'off', locale: 'en-US', timezoneId: 'Asia/Seoul', baseURL: APP });
if (SCALE) test.use({ deviceScaleFactor: Number(SCALE) });

const LIST = '[data-column-cards]';

/** Lays the rules of the run over the app's. */
async function addStyles(page: Page): Promise<void> {
  const drawAll = `${LIST} > *, ${LIST} [data-card] { content-visibility: visible !important; }`;
  const content = `${DRAW_ALL ? drawAll : ''}${CSS_RULES}`;
  if (content) await page.addStyleTag({ content });
}

/** The middle, the 95th of a hundred, and the longest of some times, rounded to a tenth. */
function summary(times: number[]): { median: number; p95: number; max: number; over50: number } {
  const sorted = [...times].sort((a, b) => a - b);
  const at = (part: number): number =>
    Math.round((sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * part))] ?? 0) * 10) /
    10;
  return {
    median: at(0.5),
    p95: at(0.95),
    max: at(1),
    over50: times.filter((time) => time > 50).length,
  };
}

/** How many long tasks there were, how long they took together, and the longest. */
function tasksOf(times: number[]): { count: number; totalMs: number; maxMs: number } {
  return {
    count: times.length,
    totalMs: times.reduce((sum, time) => sum + time, 0),
    maxMs: Math.max(0, ...times),
  };
}

/** Chromium's counters of time spent, in milliseconds, to be taken from one another. */
async function counters(cdp: CDPSession | undefined): Promise<Record<string, number>> {
  if (!cdp) return {};
  const { metrics } = await cdp.send('Performance.getMetrics');
  const wanted = ['LayoutDuration', 'RecalcStyleDuration', 'ScriptDuration', 'TaskDuration'];
  return Object.fromEntries(
    metrics
      .filter((metric) => wanted.includes(metric.name))
      .map((metric) => [metric.name, metric.value * 1000]),
  );
}

function spent(before: Record<string, number>, after: Record<string, number>): object {
  return Object.fromEntries(
    Object.entries(after).map(([name, value]) => [
      name.replace('Duration', ''),
      Math.round(value - (before[name] ?? 0)),
    ]),
  );
}

/** Records the time between frames, and the long tasks, from now until `stopFrames`. */
async function startFrames(page: Page): Promise<void> {
  await page.evaluate(() => {
    const record = { times: [] as number[], longTasks: [] as number[], going: true };
    Reflect.set(window, 'spikeFrames', record);
    if (PerformanceObserver.supportedEntryTypes.includes('longtask')) {
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) record.longTasks.push(Math.round(entry.duration));
      });
      observer.observe({ type: 'longtask' });
      Reflect.set(window, 'spikeObserver', observer);
    }
    let last = performance.now();
    const frame = (now: number): void => {
      record.times.push(now - last);
      last = now;
      if (record.going) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  });
}

async function stopFrames(page: Page): Promise<{ times: number[]; longTasks: number[] }> {
  return page.evaluate(() => {
    const record = Reflect.get(window, 'spikeFrames') as {
      times: number[];
      longTasks: number[];
      going: boolean;
    };
    record.going = false;
    (Reflect.get(window, 'spikeObserver') as PerformanceObserver | undefined)?.disconnect();
    // The first interval is the wait for the first frame, not a frame.
    return { times: record.times.slice(1), longTasks: record.longTasks };
  });
}

/** Scrolls the long column `step` pixels a frame for `frames` frames; answers how far it went. */
async function scrollBy(page: Page, step: number, frames: number): Promise<number> {
  return cardsBox(page, LONG_COLUMN).evaluate(
    (element, [by, count]) =>
      new Promise<number>((resolve) => {
        const from = element.scrollTop;
        let left = count;
        const frame = (): void => {
          if (left === 0) {
            resolve(element.scrollTop - from);
            return;
          }
          left -= 1;
          element.scrollTop += by;
          requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
      }),
    [step, frames] as const,
  );
}

/** A run of frames scrolled, with what it cost. */
async function scrollRun(
  page: Page,
  cdp: CDPSession | undefined,
  step: number,
  frames: number,
): Promise<object> {
  const before = await counters(cdp);
  await startFrames(page);
  const scrolled = await scrollBy(page, step, frames);
  const { times, longTasks } = await stopFrames(page);
  return {
    scrolled,
    frameMs: summary(times),
    longTasks: tasksOf(longTasks),
    spentMs: spent(before, await counters(cdp)),
    ...(FRAMES ? { framesMs: times.map((time) => Math.round(time)) } : {}),
  };
}

/**
 * Flings the column up from its end, a frame at a time, and answers how far
 * the cards in sight moved other than by the scroll: the most, and in how
 * many frames by more than a pixel. Not timed: finding the card in sight
 * lays the page out.
 */
async function jumps(page: Page, step: number, frames: number): Promise<object> {
  return cardsBox(page, LONG_COLUMN).evaluate(
    async (element, [by, count]) => {
      element.scrollTop = element.scrollHeight;
      const frame = (): Promise<void> =>
        new Promise((resolve) => {
          requestAnimationFrame(() => {
            resolve();
          });
        });
      await frame();
      await frame();
      const box = element.getBoundingClientRect();
      const inSight = (): HTMLElement | undefined =>
        document
          .elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)
          ?.closest<HTMLElement>('[data-card]') ?? undefined;
      let most = 0;
      let jumped = 0;
      for (let left = count; left > 0; left -= 1) {
        const card = inSight();
        const top = card?.getBoundingClientRect().top;
        const scrollBefore = element.scrollTop;
        element.scrollTop -= by;
        await frame();
        if (!card || top === undefined) continue;
        const moved = card.getBoundingClientRect().top - top;
        const off = Math.abs(moved - (scrollBefore - element.scrollTop));
        most = Math.max(most, off);
        if (off > 1) jumped += 1;
      }
      return { mostPx: Math.round(most), framesJumped: jumped };
    },
    [step, frames] as const,
  );
}

/**
 * Drags the third card down the column with the mouse and holds it at the
 * bottom edge, and answers the time between frames meanwhile; then lets it
 * go outside the board, which cancels.
 */
async function dragRun(page: Page, cdp: CDPSession | undefined): Promise<object> {
  const list = cardsBox(page, LONG_COLUMN);
  await list.evaluate((element) => {
    element.scrollTop = 0;
  });
  const card = list.locator('[data-card]').nth(2);
  const from = await card.boundingBox();
  const box = await list.boundingBox();
  if (!from || !box) throw new Error('nothing to drag');
  const x = from.x + from.width / 2;
  await page.mouse.move(x, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(x + 6, from.y + from.height / 2 + 6, { steps: 3 });
  await expect(page.locator('[data-drag-overlay]')).toBeVisible();
  const before = await counters(cdp);
  await startFrames(page);
  const bottom = box.y + box.height - 4;
  for (let step = 1; step <= 40; step += 1) {
    const y = from.y + ((bottom - from.y) * step) / 40;
    await page.mouse.move(x, y);
  }
  // Held at the edge: the column scrolls by itself, and the gap follows.
  const scrolledFrom = await list.evaluate((element) => element.scrollTop);
  await page.waitForTimeout(3000);
  const scrolled = (await list.evaluate((element) => element.scrollTop)) - scrolledFrom;
  const { times, longTasks } = await stopFrames(page);
  const result = {
    edgeScrolled: scrolled,
    frameMs: summary(times),
    longTasks: tasksOf(longTasks),
    spentMs: spent(before, await counters(cdp)),
  };
  await page.mouse.move(x, 2, { steps: 4 });
  await page.mouse.up();
  return result;
}

test('a column of some 1,600 cards, scrolled on a phone', async ({ page, browserName }, info) => {
  test.skip(!APP, 'set FVOCI_SPIKE_APP to the address of a preview of the built app');
  test.setTimeout(900_000);
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  await fitPane(page, paneOf(info));
  const path = await makeLongColumn(page, COUNT);
  await addStyles(page);

  const cdp = browserName === 'chromium' ? await page.context().newCDPSession(page) : undefined;
  await cdp?.send('Performance.enable');
  await cdp?.send('Emulation.setCPUThrottlingRate', { rate: 4 });

  const drawnMs = await showView(page, path, COUNT);
  const list = cardsBox(page, LONG_COLUMN);
  await expect(page.locator('[data-column] h3').first()).toHaveText(LONG_COLUMN);
  // A moment for whatever the drawing left to do.
  await page.waitForTimeout(1000);
  const counts = await list.evaluate((element) => {
    const heights = [...element.querySelectorAll('[data-card]')]
      .slice(0, 50)
      .map((card) => Math.round(card.getBoundingClientRect().height))
      .sort((a, b) => a - b);
    return {
      cards: element.querySelectorAll('[data-card]').length,
      columnElements: element.querySelectorAll('*').length,
      documentElements: document.querySelectorAll('*').length,
      scrollHeight: element.scrollHeight,
      cardHeights: [heights[0], heights[Math.floor(heights.length / 2)], heights.at(-1)],
    };
  });

  const slow = await scrollRun(page, cdp, 8, 240);
  const fling = await scrollRun(page, cdp, 80, 240);
  await list.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  const flingUp = await scrollRun(page, cdp, -80, 240);
  // A fresh page for the jumps: cards seen before keep the height they had.
  const drag = await dragRun(page, cdp);

  await cdp?.send('Emulation.setCPUThrottlingRate', { rate: 1 });
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  const again = await makeLongColumn(page, COUNT);
  await cdp?.send('Emulation.setCPUThrottlingRate', { rate: 4 });
  await addStyles(page);
  await showView(page, again, COUNT);
  await page.waitForTimeout(1000);
  const jumpsUp = await jumps(page, 80, 120);

  const figures = {
    project: info.project.name,
    browser: `${browserName} ${page.context().browser()?.version() ?? ''}`,
    drawAll: DRAW_ALL,
    css: CSS_RULES,
    count: COUNT,
    scale: SCALE ?? 'project',
    cpuSlowdown: cdp ? 4 : 1,
    loadAverage: loadavg().map((load) => Math.round(load * 10) / 10),
    ...counts,
    drawnMs,
    slow,
    fling,
    flingUp,
    jumpsUp,
    drag,
  };
  process.stdout.write(`[kanban-scroll] ${JSON.stringify(figures)}\n`);
  info.annotations.push({ type: 'kanban-scroll', description: JSON.stringify(figures) });
});
