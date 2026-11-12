/**
 * Formulas, diagrams and the emoji list in a real browser: that each library
 * is fetched only by a body that needs it, that both the editor and the
 * read-only view draw, in the light and in the dark, and how a formula is
 * typed and changed.
 *
 * Screenshots of both looks are written to `target/screens/spikes/editor/`
 * at the top of the repository when `FVOCI_EDITOR_SHOTS=1` is set. That
 * folder is not committed; the pictures in docs/screens/editor/ stay as
 * they are.
 */
import path from 'node:path';

import { expect, test, type Page, type Request } from '@playwright/test';

const SHOTS = path.resolve(import.meta.dirname, '../../../../target/screens/spikes/editor');
const TAKE_SHOTS = process.env.FVOCI_EDITOR_SHOTS === '1';

const editorOf = (page: Page) => page.getByTestId('editor').locator('.ProseMirror');
const viewOf = (page: Page) => page.getByTestId('view');

/** The requests a page makes, by the library they belong to. */
function watch(page: Page) {
  const urls: string[] = [];
  page.on('request', (request: Request) => urls.push(request.url()));
  const matching = (pattern: RegExp): string[] => urls.filter((url) => pattern.test(url));
  return {
    urls,
    // The formula library, its style sheet and its fonts.
    formulas: () => matching(/katex/i),
    // Not a folder that only happens to have the word in its name (a checkout called `web-mermaid`).
    diagrams: () => matching(/(?<![\w-])mermaid/i),
    emoji: () => matching(/editor\/emoji\//),
    fonts: () =>
      matching(/KaTeX_[\w-]+\.(woff2?|ttf|otf)/).map((url) => url.split('/').pop()?.split('?')[0]),
  };
}

async function stored(page: Page): Promise<unknown> {
  return JSON.parse((await page.getByTestId('stored').textContent()) ?? 'null') as unknown;
}

async function open(page: Page, query = ''): Promise<void> {
  await page.goto(`/spikes/editor/${query}`);
  await expect(editorOf(page)).toBeVisible();
}

/** Loads a sample and waits until everything in it that can be drawn has been. */
async function load(page: Page, sample: string): Promise<void> {
  await page.locator(`[data-sample="${sample}"]`).click();
  await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
  await drawn(page);
}

async function drawn(page: Page): Promise<void> {
  await expect(page.locator('.doc-math:not([data-state])')).toHaveCount(0);
  await expect(page.locator('.doc-diagram[data-state="pending"]')).toHaveCount(0, {
    timeout: 20_000,
  });
}

test.describe('what is fetched, and when', () => {
  test('a body with neither formulas nor diagrams fetches neither library, nor the emoji list', async ({
    page,
  }) => {
    const seen = watch(page);
    await open(page);
    await load(page, 'plain');
    await editorOf(page).click();
    await editorOf(page).pressSequentially(
      'plain typing, with a time 10:30 and an address http://x',
    );
    await page.waitForLoadState('networkidle');
    expect(seen.formulas()).toEqual([]);
    expect(seen.diagrams()).toEqual([]);
    expect(seen.emoji()).toEqual([]);
    // The editor itself is there, and so is the view.
    await expect(viewOf(page)).toContainText('plain typing');
  });

  test('a body with a formula fetches the formula library and not the diagram library', async ({
    page,
  }) => {
    const seen = watch(page);
    await open(page);
    await load(page, 'everything');
    await page.waitForLoadState('networkidle');
    expect(seen.formulas().length).toBeGreaterThan(0);
    expect(seen.diagrams()).toEqual([]);
    await expect(editorOf(page).locator('.doc-math[data-state="drawn"]')).toHaveCount(2);
    await expect(viewOf(page).locator('.doc-math[data-state="drawn"]')).toHaveCount(2);
  });

  test('a body with a diagram fetches the diagram library and not the formula library', async ({
    page,
  }) => {
    const seen = watch(page);
    await open(page);
    await load(page, 'diagrams');
    await page.waitForLoadState('networkidle');
    expect(seen.diagrams().length).toBeGreaterThan(0);
    expect(seen.formulas()).toEqual([]);
    await expect(viewOf(page).locator('.doc-diagram[data-state="drawn"]')).toHaveCount(4);
  });

  test('a formula fetches only the fonts it uses, as WOFF2', async ({ page }) => {
    const seen = watch(page);
    await open(page);
    await load(page, 'everything');
    await page.evaluate(() => document.fonts.ready);
    await page.waitForLoadState('networkidle');
    const fonts = [...new Set(seen.fonts())].sort();
    console.warn(`fonts fetched for "a^2 + b^2 = c^2" and an integral: ${fonts.join(', ')}`);
    // Of the twenty the library has: letters, digits and signs, and the large integral sign.
    expect(fonts).toEqual([
      'KaTeX_Main-Regular.woff2',
      'KaTeX_Math-Italic.woff2',
      'KaTeX_Size2-Regular.woff2',
    ]);
    for (const font of fonts) expect(font).toMatch(/\.woff2$/);
  });

  test('the emoji list is fetched when a name is first typed: English alone unless the app is Korean', async ({
    page,
  }) => {
    const seen = watch(page);
    await open(page);
    await editorOf(page).click();
    await editorOf(page).pressSequentially(':');
    expect(seen.emoji()).toEqual([]);
    await editorOf(page).pressSequentially('ro');
    await expect(
      page.locator('[data-suggestions="emoji"]').getByRole('option').first(),
    ).toBeVisible();
    expect(seen.emoji().map((url) => url.split('/').pop()?.split('?')[0])).toEqual(['en.json']);
    expect(seen.formulas()).toEqual([]);
    expect(seen.diagrams()).toEqual([]);
  });
});

test.describe('the list of emoji', () => {
  test('opens behind Korean, Chinese and Japanese text without a space', async ({ page }) => {
    await open(page);
    const editor = editorOf(page);
    const list = page.locator('[data-suggestions="emoji"]');
    await editor.click();

    await editor.pressSequentially('안녕:rocket');
    await expect(list.getByRole('option').first()).toHaveText('🚀 rocket');
    await expect(list).toHaveAttribute('aria-label', 'Emoji');
    await editor.press('Enter');
    await expect(list).toHaveCount(0);

    await editor.pressSequentially(' 你好:thumbs');
    await expect(list.getByRole('option').first()).toHaveText('👍 thumbs up');
    await editor.press('Tab');

    await editor.pressSequentially(' こんにちは:tada');
    await expect(list.getByRole('option').first()).toContainText('🎉');
    await editor.press('Enter');

    // And still not inside a time or an address.
    await editor.pressSequentially(' 10:30 a:rocket note:rocket');
    await expect(list).toHaveCount(0);
    expect(await stored(page)).toEqual({
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          content: [
            { type: 'text', text: '안녕🚀 你好👍 こんにちは🎉 10:30 a:rocket note:rocket' },
          ],
        },
      ],
    });
  });

  test('is searched by Korean names too in a Korean app, and shows them', async ({ page }) => {
    const seen = watch(page);
    await open(page, '?locale=ko');
    const editor = editorOf(page);
    const list = page.locator('[data-suggestions="emoji"]');
    await editor.click();

    await editor.pressSequentially('발사:로켓');
    await expect(list.getByRole('option').first()).toHaveText('🚀 로켓');
    await expect(list).toHaveAttribute('aria-label', '이모지');
    expect(
      seen
        .emoji()
        .map((url) => url.split('/').pop()?.split('?')[0])
        .sort(),
    ).toEqual(['en.json', 'ko.json']);
    await editor.press('Enter');

    // The English name still finds it, and it is shown under its Korean one.
    await editor.pressSequentially(' :rocket');
    await expect(list.getByRole('option').first()).toHaveText('🚀 로켓');
    await editor.press('Enter');
    // An underscore stands for the space in a name of two words.
    await editor.pressSequentially(' :빨간색_하트');
    await expect(list.getByRole('option').first()).toHaveText('❤️ 빨간색 하트');
    await editor.press('Escape');
    await expect(list).toHaveCount(0);
    expect(JSON.stringify(await stored(page))).toContain('발사🚀 🚀 :빨간색_하트');
  });

  test('is not searched by Korean names in an English app', async ({ page }) => {
    await open(page);
    const editor = editorOf(page);
    await editor.click();
    await editor.pressSequentially(':로켓');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('[data-suggestions="emoji"]').getByRole('option')).toHaveCount(0);
    // Enter is an ordinary Enter when there is nothing to choose.
    await editor.press('Enter');
    await editor.pressSequentially('next');
    expect(JSON.stringify(await stored(page))).toContain('":로켓"');
  });

  test('follows the arrow keys and names the chosen entry to a screen reader', async ({ page }) => {
    await open(page);
    const editor = editorOf(page);
    await editor.click();
    await editor.pressSequentially(':heart');
    const options = page.locator('[data-suggestions="emoji"]').getByRole('option');
    await expect(options.first()).toBeVisible();
    await expect(options.first()).toHaveAttribute('aria-selected', 'true');
    await editor.press('ArrowDown');
    await expect(options.nth(1)).toHaveAttribute('aria-selected', 'true');
    const active = await editor.getAttribute('aria-activedescendant');
    expect(active).toBe(await options.nth(1).getAttribute('id'));
    const chosen = (await options.nth(1).textContent()) ?? '';
    await editor.press('Enter');
    await expect(editor).not.toHaveAttribute('aria-activedescendant');
    expect(JSON.stringify(await stored(page))).toContain(Array.from(chosen)[0] ?? 'nothing');
  });
});

test.describe('formulas', () => {
  test('typing `$…$` makes a formula that both the editor and the view draw', async ({ page }) => {
    await open(page);
    const editor = editorOf(page);
    await editor.click();
    await editor.pressSequentially('넓이는 $\\pi r^2$ 입니다. 값은 $5 and $10 stays text.');
    expect(await stored(page)).toEqual({
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          content: [
            { type: 'text', text: '넓이는 ' },
            { type: 'inlineMath', attrs: { latex: '\\pi r^2' } },
            { type: 'text', text: ' 입니다. 값은 $5 and $10 stays text.' },
          ],
        },
      ],
    });
    await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
    for (const body of [editor, viewOf(page)]) {
      const formula = body.locator('.doc-math[data-state="drawn"]');
      await expect(formula).toHaveCount(1);
      await expect(formula).toHaveAttribute('aria-label', '\\pi r^2');
      await expect(formula.locator('.katex-html')).toBeVisible();
    }
  });

  test('`$$` and a space opens the place of a block formula on its line; the preview follows the source', async ({
    page,
  }) => {
    await open(page);
    const editor = editorOf(page);
    await editor.click();
    await editor.pressSequentially('$$ ');
    // In the flow of the body, on the line the formula will take: no box over the page.
    const box = editor.locator('p > .doc-place-new');
    const field = box.getByRole('textbox', { name: 'LaTeX source' });
    await expect(field).toBeFocused();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    expect(await stored(page)).toBeNull();

    await field.pressSequentially('\\frac{a}{');
    await expect(box.locator('.doc-place-problem')).toHaveText('This formula cannot be drawn.');
    await field.pressSequentially('b}');
    await expect(box.locator('.doc-math-preview .katex-html')).toBeVisible();
    await expect(box.locator('.doc-place-problem')).toBeEmpty();
    // Enter is a new line in a block formula; Ctrl+Enter closes the place.
    await field.press('Enter');
    await expect(box).toBeVisible();
    await field.press('Backspace');
    await field.press('Control+Enter');
    await expect(box).toHaveCount(0);

    expect(await stored(page)).toEqual({
      type: 'doc',
      content: [{ type: 'blockMath', attrs: { latex: '\\frac{a}{b}' } }, { type: 'paragraph' }],
    });
    // The formula is selected and the editor has the focus; the line behind it is one arrow away.
    await expect(editor).toBeFocused();
    await expect(editor.locator('.doc-math-block')).toHaveClass(/ProseMirror-selectednode/);
    await page.keyboard.press('ArrowDown');
    await page.keyboard.type('뒤');
    await expect(viewOf(page).locator('p')).toHaveText('뒤');
    await expect(viewOf(page).locator('div.doc-math[data-state="drawn"]')).toHaveCount(1);
  });

  test('a press on a formula opens its source; a changed source is drawn again; an emptied one removes it', async ({
    page,
  }) => {
    await open(page);
    await load(page, 'everything');
    const editor = editorOf(page);
    const formula = editor.locator('span.doc-math');
    // Pressed where the formula is drawn. (The element also holds its MathML twin, which is
    // clipped away to a point elsewhere and is where an automated click on the whole would land.)
    const drawing = formula.locator('.katex-html');
    await drawing.click();
    const box = page.getByRole('dialog', { name: 'Edit formula' });
    const field = box.getByRole('textbox');
    await expect(field).toHaveValue('a^2 + b^2 = c^2');
    await expect(field).toBeFocused();
    // Inside the window, right under the formula.
    const placed = await box.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return rect.left >= 0 && rect.right <= window.innerWidth && rect.top >= 0;
    });
    expect(placed).toBe(true);

    await field.fill('x^n + y^n = z^n');
    await field.press('Enter');
    await expect(box).toHaveCount(0);
    await expect(formula).toHaveAttribute('data-latex', 'x^n + y^n = z^n');
    await expect(formula).toHaveAttribute('data-state', 'drawn');
    await expect(viewOf(page).locator('span.doc-math')).toHaveAttribute(
      'aria-label',
      'x^n + y^n = z^n',
    );

    // Escape changes nothing.
    await drawing.click();
    await field.fill('never applied');
    await field.press('Escape');
    await expect(formula).toHaveAttribute('data-latex', 'x^n + y^n = z^n');

    // By keyboard: select the formula with the arrow keys, Enter opens it.
    await drawing.click();
    await field.press('Escape');
    await expect(editor).toBeFocused();
    await drawing.click();
    await box.getByRole('button', { name: 'Remove formula' }).click();
    await expect(editor.locator('span.doc-math')).toHaveCount(0);
    await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
  });

  test('a formula that cannot be drawn shows its source and says so, in the editor and in the view', async ({
    page,
  }) => {
    await open(page);
    await load(page, 'corners');
    for (const body of [editorOf(page), viewOf(page)]) {
      const failed = body.locator('.doc-math[data-state="error"]');
      await expect(failed).toHaveCount(2);
      await expect(failed.first()).toContainText('\\frac{1}{');
      await expect(failed.first().getByRole('note')).toHaveText('This formula cannot be drawn.');
      await expect(body.locator('.doc-math[data-state="drawn"]')).toHaveCount(14);
    }
  });

  test('a formula takes the colour of the text around it, in the light and in the dark', async ({
    page,
  }) => {
    await open(page);
    await load(page, 'embeds');
    const colours = () =>
      viewOf(page)
        .locator('span.doc-math .katex')
        .evaluate((formula) => ({
          formula: getComputedStyle(formula).color,
          text: getComputedStyle(formula.closest('p') ?? formula).color,
          page: getComputedStyle(document.body).backgroundColor,
        }));
    const light = await colours();
    expect(light.formula).toBe(light.text);
    await page.getByTestId('toggle-theme').click();
    await expect(page.locator('html')).toHaveClass(/dark/);
    const dark = await colours();
    expect(dark.formula).toBe(dark.text);
    expect(dark.formula).not.toBe(light.formula);
    expect(dark.page).not.toBe(light.page);
  });
});

test.describe('diagrams', () => {
  test('the view and the editor show the picture in place of the source', async ({ page }) => {
    await open(page);
    await load(page, 'diagrams');

    const view = viewOf(page);
    await expect(view.locator('.doc-diagram[data-state="drawn"]')).toHaveCount(4);
    await expect(view.locator('.doc-diagram[data-state="drawn"]').first()).toHaveAttribute(
      'role',
      'img',
    );
    // Hidden, not gone: the source of a drawn diagram stays in the page.
    await expect(view.locator('pre:has(code.language-mermaid)')).toHaveCount(6);
    // In sight are the one that could not be drawn and the one with nothing to draw yet.
    await expect(view.locator('pre:has(code.language-mermaid):visible')).toHaveCount(2);
    // Other code is untouched.
    await expect(view.locator('pre:has(code.language-ts)')).toBeVisible();

    const editor = editorOf(page);
    await expect(editor.locator('.doc-diagram[data-state="drawn"]')).toHaveCount(4);
    // Every source is the text of its block, out of sight until its place is opened;
    // the one that could not be drawn shows a copy where its picture would be.
    await expect(editor.locator('pre:has(code.language-mermaid)')).toHaveCount(6);
    await expect(editor.locator('pre:has(code.language-mermaid):visible')).toHaveCount(0);
    await expect(editor.locator('.doc-diagram-copy:visible')).toHaveCount(1);
    await expect(editor.locator('pre:has(code.language-ts)')).toBeVisible();
    await expect(editor.locator('.doc-diagram > svg').first()).toContainText('시작');
    await expect(editor.locator('.doc-diagram > svg').first()).toBeVisible();
  });

  test('a diagram that cannot be read keeps its source and says why; an empty one says it is empty, where it can be written', async ({
    page,
  }) => {
    await open(page);
    await load(page, 'diagrams');
    for (const [where, body] of [
      ['editor', editorOf(page)],
      ['view', viewOf(page)],
    ] as const) {
      const failed = body.locator('.doc-diagram[data-state="error"]');
      await expect(failed).toHaveCount(1);
      await expect(failed.getByRole('note')).toHaveText('This diagram cannot be drawn.');
      await expect(failed.locator('.doc-embed-detail')).toContainText('Parse error');
      const empty = body.locator('.doc-diagram[data-state="empty"]');
      await expect(empty).toHaveCount(1);
      if (where === 'view') await expect(empty).toBeHidden();
      // In the editor it can be pressed to be written.
      else await expect(empty).toHaveText('Empty diagram');
    }
    await expect(viewOf(page).locator('pre:has(code)', { hasText: 'A --> --> B' })).toBeVisible();
  });

  test('the picture follows the source as it is edited', async ({ page }) => {
    await open(page);
    const editor = editorOf(page);
    await editor.click();
    await editor.pressSequentially('```mermaid ');
    await editor.pressSequentially('flowchart LR');
    await editor.press('Enter');
    await editor.pressSequentially('  A[하나] --> B[둘]');
    const figure = editor.locator('.doc-diagram');
    await expect(figure).toHaveAttribute('data-state', 'drawn', { timeout: 20_000 });
    await expect(figure.locator('svg')).toContainText('하나');
    await expect(viewOf(page).locator('.doc-diagram svg')).toContainText('둘');

    await editor.press('Enter');
    await editor.pressSequentially('  B --> C[셋]');
    await expect(figure.locator('svg')).toContainText('셋', { timeout: 20_000 });
    // Broken while being typed: the notice comes, and goes when the source is whole again.
    await editor.press('Enter');
    await editor.pressSequentially('  C -->');
    await expect(figure).toHaveAttribute('data-state', 'error', { timeout: 20_000 });
    await editor.pressSequentially(' D');
    await expect(figure).toHaveAttribute('data-state', 'drawn', { timeout: 20_000 });
    expect(JSON.stringify(await stored(page))).toContain('"language":"mermaid"');
    await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
  });

  test('a diagram is drawn with the colours of the page, and again when the page turns dark', async ({
    page,
  }) => {
    await open(page);
    await load(page, 'embeds');
    const picture = viewOf(page).locator('.doc-diagram svg').first();
    const look = () =>
      picture.evaluate((svg) => {
        // Colours as the bytes they are painted with, however they are written.
        const canvas = document
          .createElement('canvas')
          .getContext('2d', { willReadFrequently: true });
        const painted = (colour: string): string => {
          if (!canvas) return '';
          canvas.clearRect(0, 0, 1, 1);
          canvas.fillStyle = colour;
          canvas.fillRect(0, 0, 1, 1);
          return Array.from(canvas.getImageData(0, 0, 1, 1).data.slice(0, 3)).join(' ');
        };
        const node = svg.querySelector('.node rect, .node polygon');
        const label = svg.querySelector('.node text, .nodeLabel');
        return {
          id: svg.id,
          fill: node ? painted(getComputedStyle(node).fill) : '',
          text: label ? painted(getComputedStyle(label).fill) : '',
          page: painted(getComputedStyle(document.body).backgroundColor),
          muted: painted(
            getComputedStyle(document.querySelector('.doc-body pre') ?? document.body)
              .backgroundColor,
          ),
        };
      });
    const light = await look();
    // Nodes are filled with the token for muted surfaces, the same as a code block.
    expect(light.fill).toBe(light.muted);

    await page.getByTestId('toggle-theme').click();
    await expect(page.locator('html')).toHaveClass(/dark/);
    await expect.poll(async () => (await look()).id).not.toBe(light.id);
    await drawn(page);
    const dark = await look();
    expect(dark.fill).toBe(dark.muted);
    expect(dark.fill).not.toBe(light.fill);
    expect(dark.text).not.toBe(light.text);
    // The editor's pictures were drawn again as well.
    await expect(editorOf(page).locator('.doc-diagram[data-state="drawn"]')).toHaveCount(3);
  });

  test('nothing is left behind in the page by drawing', async ({ page }) => {
    await open(page);
    await load(page, 'diagrams');
    await expect(page.locator('.doc-diagram-stage')).toHaveCount(0);
    // The library's temporary elements carry the identifier it was given.
    await expect(
      page.locator('body > [id^="ddoc-diagram"], body > [id^="doc-diagram"]'),
    ).toHaveCount(0);
  });
});

test.describe('both looks', () => {
  for (const look of ['light', 'dark'] as const) {
    test(`formulas and diagrams are readable in the ${look}`, async ({ page }, testInfo) => {
      await page.setViewportSize({ width: 1280, height: 900 });
      await open(page);
      if (look === 'dark') await page.getByTestId('toggle-theme').click();
      await load(page, 'embeds');
      await page.evaluate(() => document.fonts.ready);
      await page.waitForLoadState('networkidle');
      await drawn(page);

      // Readable: the text of a formula and the labels of a diagram stand out from what is behind them.
      const contrast = await viewOf(page).evaluate((view) => {
        const channel = (value: number): number => {
          const part = value / 255;
          return part <= 0.04045 ? part / 12.92 : ((part + 0.055) / 1.055) ** 2.4;
        };
        const canvas = document
          .createElement('canvas')
          .getContext('2d', { willReadFrequently: true });
        const luminance = (colour: string): number => {
          if (!canvas) return 0;
          canvas.clearRect(0, 0, 1, 1);
          canvas.fillStyle = colour;
          canvas.fillRect(0, 0, 1, 1);
          const [red = 0, green = 0, blue = 0] = canvas.getImageData(0, 0, 1, 1).data;
          return 0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue);
        };
        const ratio = (a: string, b: string): number => {
          const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
          return ((high ?? 0) + 0.05) / ((low ?? 0) + 0.05);
        };
        const page = getComputedStyle(document.body).backgroundColor;
        const formula = view.querySelector('.doc-math .katex');
        const node = view.querySelector('.doc-diagram .node rect, .doc-diagram .node polygon');
        const label = view.querySelector('.doc-diagram .node text');
        const edge = view.querySelector('.doc-diagram .flowchart-link');
        return {
          formula: formula ? ratio(getComputedStyle(formula).color, page) : 0,
          label:
            label && node ? ratio(getComputedStyle(label).fill, getComputedStyle(node).fill) : 0,
          edge: edge ? ratio(getComputedStyle(edge).stroke, page) : 0,
        };
      });
      console.warn(
        `${look}: contrast of a formula ${contrast.formula.toFixed(1)}, a node's label ${contrast.label.toFixed(1)}, an edge ${contrast.edge.toFixed(1)}`,
      );
      expect(contrast.formula).toBeGreaterThanOrEqual(4.5);
      expect(contrast.label).toBeGreaterThanOrEqual(4.5);
      expect(contrast.edge).toBeGreaterThanOrEqual(3);

      test.skip(
        !TAKE_SHOTS || testInfo.project.name !== 'desktop',
        'screenshots are taken on request, on the desktop project',
      );
      await viewOf(page).screenshot({ path: path.join(SHOTS, `view-${look}.png`) });
      await page.getByTestId('editor').screenshot({ path: path.join(SHOTS, `editor-${look}.png`) });
    });
  }
});

test.describe('images', () => {
  test('a pasted picture is uploaded; its place is held meanwhile; the document gets the identifier', async ({
    page,
  }) => {
    await open(page, '?upload-delay=600');
    const editor = editorOf(page);
    await editor.click();
    await editor.pressSequentially('before ');
    await editor.evaluate((element) => {
      const data = new DataTransfer();
      data.items.add(
        new File([new Uint8Array([137, 80, 78, 71])], 'chart.png', { type: 'image/png' }),
      );
      element.dispatchEvent(
        new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
      );
    });
    await expect(editor.locator('.doc-upload')).toHaveCount(1);
    await expect(
      page.getByRole('status').filter({ hasText: 'Uploading the image…' }),
    ).toBeVisible();
    expect(JSON.stringify(await stored(page))).not.toContain('image');

    await expect(editor.locator('img[data-file-id="file-1"]')).toBeVisible();
    await expect(editor.locator('.doc-upload')).toHaveCount(0);
    expect(JSON.stringify(await stored(page))).toContain(
      '{"type":"image","attrs":{"fileId":"file-1"}}',
    );
    await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
    await expect(viewOf(page).locator('img[data-file-id="file-1"]')).toBeVisible();
  });
});
