/**
 * The pictures of docs/calendar-proposal.md. Not a test: it writes PNG files
 * into `target/screens/spikes/calendar/` at the top of the repository (not
 * committed; the pictures in docs/calendar/ stay as they are), and runs only
 * when asked for.
 *
 *   FVOCI_CALENDAR_SHOTS=1 pnpm --filter web run spikes:e2e calendar/shots --project desktop
 *
 * Every screen and proposal is taken at a phone (360×740, touch, two device
 * pixels to one) and at a desktop (1280×800, mouse). The sizes in between
 * (the inner screen of a folding phone both ways up, a tablet upright; rough
 * figures, as in the mobile proposals) are taken where the layout differs
 * there (`MORE`).
 *
 * The same input gives the same picture: the data is written out, its "now"
 * with it, the browser's clock is fixed, and a picture is taken only after
 * the fonts have arrived, on a page where nothing animates (`still`).
 */
import { mkdirSync } from 'node:fs';
import path from 'node:path';

import { expect, test, type BrowserContextOptions, type Page } from '@playwright/test';

import { SCREENS, SIZES, addressOf, type ScreenId, type Variant } from './params.ts';
import { NOW, PAGE, settled } from './support.ts';

const ENABLED = process.env.FVOCI_CALENDAR_SHOTS === '1';
const OUT = path.resolve(import.meta.dirname, '../../../../target/screens/spikes/calendar');

type Size = keyof typeof SIZES;

const FINGER = { deviceScaleFactor: 2, hasTouch: true, isMobile: true };
const DEVICES: Record<Size, BrowserContextOptions> = {
  360: { viewport: SIZES[360], ...FINGER },
  626: { viewport: SIZES[626], ...FINGER },
  820: { viewport: SIZES[820], ...FINGER },
  890: { viewport: SIZES[890], ...FINGER },
  1280: { viewport: SIZES[1280], deviceScaleFactor: 1, hasTouch: false, isMobile: false },
};

interface Shot {
  /** The file, without `.png`. */
  name: string;
  screen: ScreenId;
  variant: Variant;
  size: Size;
  /** More of the address: `&open=0`, `&theme=dark`, ... */
  extra: string;
}

/** Pictures beyond the two sizes of every screen and proposal. */
const MORE: [ScreenId, Variant, Size, tail: string, extra: string][] = [
  // The month where bars first fit, on a tablet, and in a short wide window with and without the sidebar.
  ['month', 'a', 626, '', ''],
  ['month', 'a', 820, '', ''],
  ['month', 'a', 890, '', ''],
  ['month', 'a', 890, 'strip', '&sidebar=strip'],
  // Dark, and bars in the colour of the status.
  ['month', 'a', 360, 'dark', '&theme=dark'],
  ['month', 'b', 360, 'dark', '&theme=dark'],
  ['month', 'a', 1280, 'dark', '&theme=dark'],
  ['month', 'a', 1280, 'tint', '&tint=1'],
  ['month', 'a', 1280, 'tint-dark', '&tint=1&theme=dark'],
  // The notch and the home indicator; the line a screen may fold along.
  ['month', 'a', 360, 'safe', '&safe=1'],
  ['month', 'a', 890, 'fold', '&fold=1'],
  ['more', 'a', 890, 'fold', '&fold=1'],
  // The cards of a day with a finger: the whole cell is the button.
  ['more', 'a', 626, '', ''],
  ['more', 'b', 626, '', ''],
  ['more', 'a', 820, '', ''],
  ['more', 'b', 890, '', ''],
  // Moving a card with a finger where there are bars.
  ['move', 'a', 820, '', ''],
  ['move', 'b', 820, '', ''],
  ['move', 'b', 360, 'menu', '&open=menu'],
  ['move', 'b', 1280, 'menu', '&open=menu'],
  // Making a card with a finger where there are bars.
  ['create', 'a', 820, '', ''],
  ['create', 'b', 820, '', ''],
  // Cards without a date: closed, and the list of properties to place cards by.
  ['display', 'a', 360, 'closed', '&open=0'],
  ['display', 'b', 360, 'closed', '&open=0'],
  ['display', 'a', 360, 'by', '&open=by'],
  ['display', 'a', 1280, 'by', '&open=by'],
  ['display', 'a', 626, '', ''],
  // A week on a tablet, where a bar is large enough for a finger.
  ['week', 'a', 820, '', ''],
  ['week', 'a', 626, '', ''],
  ['range', 'a', 626, '', ''],
  // Saturday as the first day, the third a reader can choose.
  ['regional', 'a', 1280, 'saturday', '&week=saturday'],
];

const SHOTS: Shot[] = [
  ...SCREENS.flatMap((screen) =>
    (Object.keys(screen.variants) as Variant[]).flatMap((variant) =>
      ([360, 1280] as const).map((size) => ({
        name: `${screen.id}-${variant}-${size}`,
        screen: screen.id,
        variant,
        size,
        extra: '',
      })),
    ),
  ),
  ...MORE.map(([screen, variant, size, tail, extra]) => ({
    name: [screen, variant, size, tail].filter(Boolean).join('-'),
    screen,
    variant,
    size,
    extra,
  })),
];

/**
 * Nothing on the page animates or fades. What opens with the page (a popover,
 * a sheet) otherwise grows into place, and the browser may keep drawing it
 * from the bitmap it made while it was still a little smaller: its rounded
 * corner then differs by a few pixels from one run to the next.
 */
async function still(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const style = document.createElement('style');
    style.textContent =
      '*, *::before, *::after { animation: none !important; transition: none !important; }';
    // As soon as there is a head, which is before the page's own script runs.
    document.addEventListener(
      'readystatechange',
      () => {
        document.head.append(style);
      },
      { once: true },
    );
  });
}

// A trace would only slow this down, and a second attempt would hide a picture that differs.
test.use({ trace: 'off' });

test.describe('screenshots of the calendar proposals', () => {
  test('nothing is written unless asked for', () => {
    test.skip(!ENABLED, 'set FVOCI_CALENDAR_SHOTS=1 to take the screenshots');
    mkdirSync(OUT, { recursive: true });
  });

  // Each picture makes its own device; one project is enough to run them.
  test.skip(({ hasTouch }) => hasTouch, 'run with --project desktop');

  for (const shot of ENABLED ? SHOTS : []) {
    test(shot.name, async ({ browser, baseURL }) => {
      const context = await browser.newContext({
        ...DEVICES[shot.size],
        baseURL,
        locale: 'ko-KR',
        timezoneId: 'Asia/Seoul',
        colorScheme: 'light',
      });
      const page = await context.newPage();
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.clock.setFixedTime(NOW);
      await still(page);

      await page.goto(PAGE + addressOf(shot.screen, shot.variant, shot.extra));
      await expect(page.getByTestId('calendar-toolbar')).toBeVisible();
      // The rule that stops animations was there before anything opened.
      expect(await page.evaluate(() => document.getAnimations().length)).toBe(0);
      await settled(page);
      // Whatever opened with the page took the focus, and a page nobody has
      // touched yet draws the ring of the keyboard around it. A person who
      // opened it with a finger or the mouse would not see that ring.
      await page.evaluate(() => {
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      });
      await settled(page);

      await page.screenshot({
        path: path.join(OUT, `${shot.name}.png`),
        animations: 'disabled',
        caret: 'hide',
      });
      expect(errors).toEqual([]);
      await context.close();
    });
  }
});

/**
 * What a finger on the middle of the screen would hit, on the two sizes wide
 * enough to be an unfolded screen. Printed, not judged: the table in
 * docs/calendar-proposal.md is written from it.
 *
 *   FVOCI_CALENDAR_SHOTS=1 pnpm --filter web run spikes:e2e calendar/shots --project desktop -g "middle line"
 */
test.describe('what lies on the middle line of the calendar', () => {
  test.skip(({ hasTouch }) => hasTouch, 'run with --project desktop');

  for (const size of ENABLED ? ([890, 1280] as const) : []) {
    test(`the middle line at ${size}`, async ({ browser, baseURL }) => {
      test.setTimeout(120_000);
      const context = await browser.newContext({
        ...DEVICES[size],
        baseURL,
        locale: 'ko-KR',
        timezoneId: 'Asia/Seoul',
      });
      const page = await context.newPage();
      await page.clock.setFixedTime(NOW);

      for (const screen of SCREENS) {
        for (const variant of Object.keys(screen.variants) as Variant[]) {
          await page.goto(PAGE + addressOf(screen.id, variant));
          await expect(page.getByTestId('calendar-toolbar')).toBeVisible();
          await settled(page);
          const found = await page.evaluate(() => {
            const middle = window.innerWidth / 2;
            const seen = new Map<Element, string>();
            // Every fourth pixel down the line: whatever is on top there, and
            // the control it belongs to, if any.
            for (let y = 2; y < window.innerHeight; y += 4) {
              const control = document
                .elementFromPoint(middle, y)
                ?.closest('button, input, [role="button"], [role="tab"], [role="option"]');
              if (!control || seen.has(control)) continue;
              const box = control.getBoundingClientRect();
              const name =
                control.getAttribute('aria-label') ??
                control.getAttribute('placeholder') ??
                (control.textContent.trim().replace(/\s+/g, ' ').slice(0, 24) || control.tagName);
              const kind = control.matches('input') ? 'field' : 'button';
              seen.set(
                control,
                `${name} [${kind}, ${Math.round(box.left)}–${Math.round(box.right)}]`,
              );
            }
            return [...seen.values()];
          });
          console.warn(`${size} ${screen.id} ${variant}: ${found.join('; ') || 'nothing'}`);
        }
      }
      await context.close();
    });
  }
});
