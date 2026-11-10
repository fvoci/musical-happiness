/**
 * The pictures of docs/mobile-proposal.md. Not a test: it writes PNG files
 * into docs/mobile/, and runs only when asked for.
 *
 *   FVOCI_MOBILE_SHOTS=1 pnpm --filter web run spikes:e2e shots --project desktop
 *
 * Every screen and proposal is taken at two sizes: a phone (360×740, touch,
 * two device pixels to one) and a desktop (1280×800, mouse). The same input
 * gives the same picture: the data is written out, the clock is fixed, and a
 * picture is taken only after the fonts have arrived and nothing moves.
 */
import { mkdirSync } from 'node:fs';
import path from 'node:path';

import { expect, test, type BrowserContextOptions } from '@playwright/test';

import { SCREENS, addressOf, type ScreenId, type Variant } from './params.ts';
import { DESKTOP, NOW, PAGE, PHONE, settled } from './support.ts';

const ENABLED = process.env.FVOCI_MOBILE_SHOTS === '1';
const OUT = path.resolve(import.meta.dirname, '../../docs/mobile');

type Size = 360 | 820 | 1280;

const DEVICES: Record<Size, BrowserContextOptions> = {
  360: { viewport: PHONE, deviceScaleFactor: 2, hasTouch: true, isMobile: true },
  // A tablet held upright: wide enough for the fixed sidebar, and a finger.
  820: {
    viewport: { width: 820, height: 1180 },
    deviceScaleFactor: 2,
    hasTouch: true,
    isMobile: true,
  },
  1280: { viewport: DESKTOP, deviceScaleFactor: 1, hasTouch: false, isMobile: false },
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

/** What the plain picture of a screen needs to show its point. */
const ALWAYS: Partial<Record<`${ScreenId}-${Variant}-${Size}`, string>> = {
  // Scrolled sideways, so that the column that stays put is seen to stay put.
  'table-a-360': '&x=82',
};

/** Pictures beyond the two sizes of every screen and proposal. */
const MORE: [ScreenId, Variant, Size, tail: string, extra: string][] = [
  ['sidebar', 'a', 360, 'closed', '&open=0'],
  ['sidebar', 'b', 360, 'closed', '&open=0'],
  ['header', 'a', 360, 'more', '&open=more'],
  ['header', 'a', 360, 'views', '&open=views'],
  ['kanban', 'a', 360, 'dark', '&theme=dark'],
  ['card', 'a', 360, 'dark', '&theme=dark'],
  ['menu-date', 'a', 360, 'dark', '&theme=dark'],
  ['kanban', 'a', 1280, 'dark', '&theme=dark'],
  ['card', 'a', 360, 'safe', '&safe=1'],
  ['menu-card', 'a', 360, 'safe', '&safe=1'],
  ['menu-select', 'a', 360, 'keyboard', '&keyboard=1'],
  ['menu-select', 'b', 360, 'keyboard', '&keyboard=1'],
  ['kanban', 'a', 820, '', ''],
  ['menu-select', 'a', 820, '', ''],
];

const SHOTS: Shot[] = [
  ...SCREENS.flatMap((screen) =>
    (Object.keys(screen.variants) as Variant[]).flatMap((variant) =>
      ([360, 1280] as const).map((size) => ({
        name: `${screen.id}-${variant}-${size}`,
        screen: screen.id,
        variant,
        size,
        extra: ALWAYS[`${screen.id}-${variant}-${size}`] ?? '',
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

// A trace would only slow this down, and a second attempt would hide a picture that differs.
test.use({ trace: 'off' });

test.describe('screenshots of the mobile proposals', () => {
  test('nothing is written unless asked for', () => {
    test.skip(!ENABLED, 'set FVOCI_MOBILE_SHOTS=1 to take the screenshots');
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

      await page.goto(PAGE + addressOf(shot.screen, shot.variant, shot.extra));
      await expect(page.getByTestId('view-header')).toBeVisible();
      await settled(page);
      // Whatever opened with the page took the focus, and a page nobody has
      // touched yet draws the ring of the keyboard around it. A person who
      // opened it with a finger or the mouse would not see that ring.
      await page.evaluate(() => {
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      });

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
