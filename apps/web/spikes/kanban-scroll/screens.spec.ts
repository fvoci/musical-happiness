/**
 * Pictures of the long column (measure.spec.ts has how to run it), on a
 * phone's width and a wide window, light and dark: scrolled far down, with
 * one card selected and the next one focused, so that the rings around a
 * face show whole. Into `target/kanban-scroll/screens/` at the top of the
 * repository, not committed.
 */
import path from 'node:path';

import { expect, test } from '@playwright/test';

import { primaryName } from '../../e2e/keyboard.support.ts';
import { card, cardsBox, fitPane } from '../../e2e/kanban.support.ts';
import { LONG, LONG_COLUMN, makeLongColumn, showView } from '../../e2e/kanban-long.support.ts';

const APP = process.env.FVOCI_SPIKE_APP;
const SCREENS = path.resolve(import.meta.dirname, '../../../../target/kanban-scroll/screens');

test.use({ trace: 'off', locale: 'en-US', timezoneId: 'Asia/Seoul', baseURL: APP });

for (const width of [390, 1280] as const) {
  for (const scheme of ['light', 'dark'] as const) {
    test(`the long column, ${String(width)} wide, ${scheme}`, async ({ page, browserName }) => {
      test.skip(!APP, 'set FVOCI_SPIKE_APP to the address of a preview of the built app');
      test.setTimeout(300_000);
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width, height: 844 });
      await page.goto('/');
      await expect(page.getByTestId('board-page')).toBeVisible();
      // A wide window has the sidebar; the board is as wide as what is left.
      await fitPane(page, width === 390 ? 390 : width - 256);
      await showView(page, await makeLongColumn(page), LONG);
      const list = cardsBox(page, LONG_COLUMN);
      // Far down: the column draws a window there, after a blank for the cards above.
      await list.evaluate((element) => {
        element.scrollTop = element.scrollHeight * 0.6;
      });
      // The first card wholly in sight, once the window has been drawn there.
      const firstInSight = (): Promise<string> =>
        list.evaluate((element) => {
          const top = element.getBoundingClientRect().top;
          const first = [...element.querySelectorAll('[data-card]')].find(
            (each) => each.getBoundingClientRect().top >= top,
          );
          return first?.querySelector('[data-slot="card-title-text"]')?.textContent.trim() ?? '';
        });
      await expect.poll(firstInSight).toMatch(/^Long \d{3,}$/);
      const title = await firstInSight();
      const selected = card(page, title);
      await selected.click({ modifiers: [await primaryName(page)] });
      await expect(selected).toHaveAttribute('data-selected');
      await selected.focus();
      await page.keyboard.press('Tab');
      await page.keyboard.press('Tab');
      await expect(list.locator('[data-card]:focus-visible')).toHaveCount(1);
      await page.screenshot({
        path: path.join(SCREENS, `long-column-${String(width)}-${scheme}-${browserName}.png`),
      });
    });
  }
}
