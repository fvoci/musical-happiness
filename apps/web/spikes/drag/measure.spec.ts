/**
 * How long a frame takes while a card is dragged over a long column. Not a
 * test: it prints numbers, and runs only when asked for.
 *
 *   FVOCI_SPIKE_MEASURE=100,500,1000 pnpm run spikes:e2e measure --project desktop
 */
import { expect, test } from '@playwright/test';

/** Cards per column, from the variable: `FVOCI_SPIKE_MEASURE=100,500,1000`. */
const SIZES = (process.env.FVOCI_SPIKE_MEASURE ?? '')
  .split(',')
  .map(Number)
  .filter((size) => size > 0);

/** What the page keeps for this file. Times are in milliseconds. */
interface Recorded {
  spikeFrames: number[];
  /** From the button going up to the frame after it. */
  spikeDropped: Promise<number>;
}

// Recording a trace copies the whole document at every step, which is what
// would be measured.
test.use({ trace: 'off' });

test.describe('frame times while dragging', () => {
  test('nothing is measured unless sizes are given', () => {
    test.skip(SIZES.length === 0, 'set FVOCI_SPIKE_MEASURE to the sizes to measure');
  });

  test.skip(({ hasTouch }) => hasTouch, 'measured with the mouse');

  for (const size of SIZES) {
    test(`two columns of ${size} cards`, async ({ page }) => {
      test.setTimeout(120_000);
      await page.goto(`/spikes/drag/?sizes=${size},${size}`);
      const first = page.locator('[data-card="backlog-1"]');
      await expect(first).toBeVisible();
      const from = await first.boundingBox();
      const column = await page.locator('[data-drag-list="ready"]').boundingBox();
      if (!from || !column) throw new Error('the board is not on the page');

      await page.mouse.move(from.x + 40, from.y + 20);
      await page.mouse.down();
      await page.mouse.move(from.x + 50, from.y + 30, { steps: 3 });
      await expect(page.locator('[data-drag-overlay]')).toBeVisible();

      // Records the time between frames from here on.
      await page.evaluate(() => {
        const frames: number[] = [];
        let last = performance.now();
        const onFrame = (now: number): void => {
          frames.push(now - last);
          last = now;
          requestAnimationFrame(onFrame);
        };
        requestAnimationFrame(onFrame);
        Object.assign(window, { spikeFrames: frames });
      });

      // Up and down the other column, outside the zones that scroll, one
      // move per frame: the target changes on most of them.
      const x = column.x + column.width / 2;
      const top = column.y + 80;
      const bottom = column.y + column.height - 80;
      for (let step = 0; step < 120; step += 1) {
        const phase = (step % 40) / 40;
        const along = phase < 0.5 ? phase * 2 : 2 - phase * 2;
        await page.mouse.move(x, top + (bottom - top) * along);
        await page.evaluate(
          () =>
            new Promise((resolve) => {
              requestAnimationFrame(resolve);
            }),
        );
      }
      const frames = await page.evaluate(() =>
        (globalThis as unknown as Recorded).spikeFrames.slice(2),
      );

      // Held at the foot of the column: every frame scrolls and measures again.
      await page.mouse.move(x, column.y + column.height - 4);
      await page.evaluate(() => {
        (globalThis as unknown as Recorded).spikeFrames.length = 0;
      });
      await page.waitForTimeout(2000);
      const scrolling = await page.evaluate(() =>
        (globalThis as unknown as Recorded).spikeFrames.slice(2),
      );

      // From the button going up to the first frame that shows the new order.
      await page.evaluate(() => {
        const dropped = new Promise<number>((resolve) => {
          window.addEventListener(
            'pointerup',
            () => {
              const start = performance.now();
              requestAnimationFrame(() => {
                setTimeout(() => {
                  resolve(performance.now() - start);
                }, 0);
              });
            },
            { capture: true, once: true },
          );
        });
        Object.assign(window, { spikeDropped: dropped });
      });
      await page.mouse.up();
      const dropTime = await page.evaluate(() => (globalThis as unknown as Recorded).spikeDropped);
      await expect(page.getByTestId('moves')).toHaveText('1');

      const summary = (values: number[]): string => {
        const sorted = [...values].sort((a, b) => a - b);
        const at = (share: number): string =>
          (sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * share))] ?? 0).toFixed(1);
        const slow = values.filter((value) => value > 20).length;
        return `median ${at(0.5)} ms, 95% ${at(0.95)} ms, longest ${at(1)} ms, ${slow} of ${values.length} frames over 20 ms`;
      };
      console.warn(`${size} cards per column, moving:   ${summary(frames)}`);
      console.warn(`${size} cards per column, scrolling: ${summary(scrolling)}`);
      console.warn(`${size} cards per column, drop:      ${dropTime.toFixed(1)} ms`);
    });
  }
});
