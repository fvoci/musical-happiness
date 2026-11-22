/**
 * What one letter from the server costs an editor that holds a body at the limit of
 * nodes, with the content check on and off: printed as a row of JSON for each, and
 * held only to have arrived without a content error. Run alone, with nothing else on
 * the machine where the times are to be compared:
 *
 *   pnpm --filter web run spikes:e2e --project remote-chromium
 */
import { expect, test } from '@playwright/test';

import type { RemoteSpike } from './api.ts';

/**
 * The page's handle: `window.spike`, of the type `./api.ts` declares. No global says so,
 * as the collaboration spike's `Window.spike` is of another.
 */
type Handled = { spike: RemoteSpike };

/** The letters measured, and those sent before them that are not. */
const RUNS = 9;
const WARM = 2;

for (const shape of ['paragraphs', 'marks']) {
  for (const check of [true, false]) {
    test(`a letter from the server into ${shape}, content check ${check ? 'on' : 'off'}`, async ({
      page,
      browserName,
    }) => {
      // A body at the limit is drawn and measured in tens of seconds on a busy machine.
      test.setTimeout(300_000);
      await page.goto(`/spikes/remote/?shape=${shape}&check=${check ? 1 : 0}`);
      const opened = await page.evaluate(() => (window as unknown as Handled).spike.ready);
      const measured = await page.evaluate(
        ([runs, warm]) => (window as unknown as Handled).spike.measure(runs, warm),
        [RUNS, WARM] as const,
      );
      const alone = await page.evaluate((runs) => {
        const spike = (window as unknown as Handled).spike;
        return { checked: spike.checked(runs), stored: spike.stored(runs) };
      }, RUNS);
      const contentErrors = await page.evaluate(() =>
        (window as unknown as Handled).spike.contentErrors(),
      );
      process.stdout.write(
        `${JSON.stringify({ browserName, shape, ...opened, ...measured, ...alone })}\n`,
      );
      expect(measured.arrived, 'every letter is in the editor').toBe(true);
      expect(contentErrors).toEqual([]);
    });
  }
}
