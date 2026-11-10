/**
 * The pictures of docs/mobile-proposal.md. Not a test: it writes PNG files
 * into docs/mobile/, and runs only when asked for.
 *
 *   FVOCI_MOBILE_SHOTS=1 pnpm --filter web run spikes:e2e shots --project desktop
 *
 * Every screen and proposal is taken at two sizes: a phone (360×740, touch,
 * two device pixels to one) and a desktop (1280×800, mouse). Four sizes in
 * between are taken only where the layout differs there (`MORE`): the outer
 * and the inner screen of a folding phone, the inner one both ways up, and a
 * tablet held upright. Those four are rough figures, not measured devices.
 *
 * The same input gives the same picture: the data is written out, the clock
 * is fixed, and a picture is taken only after the fonts have arrived and
 * nothing moves.
 */
import { mkdirSync } from 'node:fs';
import path from 'node:path';

import { expect, test, type BrowserContextOptions } from '@playwright/test';

import { SCREENS, SIZES, addressOf, type ScreenId, type Variant } from './params.ts';
import { DESKTOP, NOW, PAGE, settled } from './support.ts';

const ENABLED = process.env.FVOCI_MOBILE_SHOTS === '1';
const OUT = path.resolve(import.meta.dirname, '../../docs/mobile');

type Size = keyof typeof SIZES;

const FINGER = { deviceScaleFactor: 2, hasTouch: true, isMobile: true };
const DEVICES: Record<Size, BrowserContextOptions> = {
  360: { viewport: SIZES[360], ...FINGER },
  466: { viewport: SIZES[466], ...FINGER },
  626: { viewport: SIZES[626], ...FINGER },
  820: { viewport: SIZES[820], ...FINGER },
  890: { viewport: SIZES[890], ...FINGER },
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
  // Compact, and wider than a phone: one column still. With 84 pixels kept at the right edge only.
  ['kanban', 'a', 466, '', ''],
  ['kanban', 'a', 466, 'safe', '&safe=right'],
  // Medium: a strip or nothing where the sidebar was, two columns, a card as a panel.
  ['sidebar', 'a', 626, '', ''],
  ['sidebar', 'b', 626, '', ''],
  ['sidebar', 'b', 626, 'closed', '&open=0'],
  ['kanban', 'a', 626, '', ''],
  ['card', 'a', 626, '', ''],
  ['kanban', 'a', 820, '', ''],
  ['menu-select', 'a', 820, '', ''],
  // Expanded, but short and not very wide: what the pane has depends on the sidebar.
  ['kanban', 'a', 890, '', ''],
  ['kanban', 'a', 890, 'strip', '&sidebar=strip'],
  ['card', 'a', 890, '', ''],
  ['card', 'b', 890, '', ''],
  // The line down the middle, where such a screen may fold.
  ['card', 'a', 890, 'fold', '&fold=1'],
  ['card', 'b', 890, 'fold', '&fold=1'],
  ['sidebar', 'b', 890, 'fold', '&fold=1'],
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

/**
 * What a finger on the middle of the screen would hit, on the two sizes wide
 * enough to be an unfolded screen. Printed, not judged: the table in
 * docs/mobile-proposal.md is written from it.
 *
 *   FVOCI_MOBILE_SHOTS=1 pnpm --filter web run spikes:e2e shots --project desktop -g "middle line"
 */
test.describe('what lies on the middle line', () => {
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
          await expect(page.getByTestId('view-header')).toBeVisible();
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
