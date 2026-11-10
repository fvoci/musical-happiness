/**
 * The read-only view next to the editor, in a real browser: the same document
 * has to come out looking the same in both.
 *
 * tests/ui/body-compare.test.ts compares the two trees of elements, without
 * layout. Here the browser lays both out and every element is measured: where
 * it is inside its body, how large it is, and the computed styles that decide
 * how it looks.
 *
 * Left out, and nothing else:
 *
 *  - The elements the editor adds for its cursor and for the names of
 *    checkboxes: the separator images, the line break behind a mention that
 *    ends a paragraph, and the hidden `<span>` in the label of a task. (The
 *    same ones the other test leaves out; their numbers 3, 4 and 8.)
 *  - How white space is treated in `<label>`, `<input>`, `<hr>` and `<img>`.
 *    The editor marks these as not editable, and its style sheet resets
 *    `white-space` on whatever is. None of them holds text.
 */
import { expect, test, type Page } from '@playwright/test';

/** The computed styles that are compared, element by element. */
const STYLES = [
  'display',
  'font-family',
  'font-size',
  'font-weight',
  'font-style',
  'font-variant-ligatures',
  'font-feature-settings',
  'line-height',
  'letter-spacing',
  'color',
  'background-color',
  'text-decoration-line',
  'text-align',
  'vertical-align',
  'white-space-collapse',
  'text-wrap-mode',
  'overflow-wrap',
  'word-break',
  'margin-top',
  'margin-right',
  'margin-bottom',
  'margin-left',
  'padding-top',
  'padding-right',
  'padding-bottom',
  'padding-left',
  'border-top-width',
  'border-right-width',
  'border-bottom-width',
  'border-left-width',
  'border-top-color',
  'border-left-color',
  'border-top-left-radius',
  'list-style-type',
  'accent-color',
  'overflow-x',
];

/** See the top: elements without text, and the styles that only matter for text. */
const WITHOUT_TEXT = new Set(['label', 'input', 'hr', 'img']);
const WHITE_SPACE = new Set(['white-space-collapse', 'text-wrap-mode']);

interface Measured {
  tag: string;
  text: string;
  checked: boolean | null;
  box: string;
  styles: Record<string, string>;
}

const editorOf = (page: Page) => page.getByTestId('editor').locator('.ProseMirror');
const viewOf = (page: Page) => page.getByTestId('view').locator('.doc-body');

/** Every element of a drawn body, in document order, with its place, size and styles. */
async function measure(page: Page, which: 'editor' | 'view'): Promise<Measured[]> {
  const root = which === 'editor' ? editorOf(page) : viewOf(page);
  return root.evaluate((body, styles) => {
    const origin = body.getBoundingClientRect();
    const round = (value: number): number => Math.round(value * 100) / 100;
    const isSeparator = (node: Node | null): boolean =>
      node instanceof HTMLElement && node.classList.contains('ProseMirror-separator');
    const measured: Measured[] = [];
    for (const element of [body, ...Array.from(body.querySelectorAll('*'))]) {
      // What the editor adds for the cursor and for the name of a checkbox.
      if (isSeparator(element)) continue;
      if (element.tagName === 'SPAN' && element.parentElement?.tagName === 'LABEL') continue;
      if (element.tagName === 'BR' && element.classList.contains('ProseMirror-trailingBreak')) {
        let before = element.previousSibling;
        while (isSeparator(before)) before = before?.previousSibling ?? null;
        if (before instanceof HTMLElement && before.hasAttribute('data-mention')) continue;
      }
      const rect = element.getBoundingClientRect();
      const computed = getComputedStyle(element);
      measured.push({
        tag: element.tagName.toLowerCase(),
        text: Array.from(element.childNodes)
          .filter((node) => node.nodeType === 3)
          .map((node) => node.nodeValue)
          .join('|'),
        checked: element instanceof HTMLInputElement ? element.checked : null,
        box: [rect.x - origin.x, rect.y - origin.y, rect.width, rect.height].map(round).join(' '),
        styles: Object.fromEntries(styles.map((name) => [name, computed.getPropertyValue(name)])),
      });
    }
    return measured;
  }, STYLES);
}

/** Where two measurements differ, in words; empty when they do not. */
function differences(editor: Measured[], view: Measured[]): string[] {
  const found = new Set<string>();
  if (editor.length !== view.length)
    found.add(`${editor.length} elements in the editor, ${view.length} in the view`);
  editor.forEach((expected, index) => {
    const actual = view[index];
    if (!actual) return;
    const where = `#${index} <${expected.tag}> "${expected.text.slice(0, 24)}"`;
    if (actual.tag !== expected.tag) found.add(`${where}: the view has <${actual.tag}>`);
    if (actual.text !== expected.text) found.add(`${where}: text "${actual.text.slice(0, 24)}"`);
    if (actual.checked !== expected.checked) found.add(`${where}: checked ${actual.checked}`);
    if (actual.box !== expected.box) found.add(`${where}: box ${expected.box} / ${actual.box}`);
    for (const [name, value] of Object.entries(expected.styles)) {
      if (WITHOUT_TEXT.has(expected.tag) && WHITE_SPACE.has(name)) continue;
      if (actual.styles[name] !== value)
        found.add(`<${expected.tag}> ${name}: ${value} / ${actual.styles[name] ?? ''}`);
    }
  });
  return [...found];
}

/** Waits until every picture of both bodies has loaded or is known to have no address. */
async function picturesLoaded(page: Page): Promise<void> {
  for (const body of [editorOf(page), viewOf(page)]) {
    const pictures = body.locator('img[src]');
    for (let index = 0; index < (await pictures.count()); index += 1) {
      const picture = pictures.nth(index);
      await picture.scrollIntoViewIfNeeded();
      await expect
        .poll(() => picture.evaluate((element: HTMLImageElement) => element.naturalWidth))
        .toBeGreaterThan(0);
    }
  }
}

test.beforeEach(async ({ page }) => {
  await page.goto('/spikes/editor/');
  await expect(editorOf(page)).toBeVisible();
});

for (const sample of ['everything', 'corners']) {
  test(`the view lays out the sample "${sample}" like the editor`, async ({ page }) => {
    await page.locator(`[data-sample="${sample}"]`).click();
    await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
    await picturesLoaded(page);
    // The page draws with the app's fonts, which arrive while it settles: the
    // rules of a family are fetched when the browser is idle, a file when a
    // character first needs it. A measurement taken across such an arrival
    // compares one side in the fallback font with the other in the real one.
    await page.waitForLoadState('networkidle');
    await page.evaluate(() => document.fonts.ready);

    await expect(async () => {
      const editor = await measure(page, 'editor');
      const view = await measure(page, 'view');
      // Nothing moved while the two were measured.
      expect(await measure(page, 'editor')).toEqual(editor);
      // Enough went through the comparison to mean something.
      expect(editor.length).toBeGreaterThan(sample === 'corners' ? 200 : 40);
      expect(differences(editor, view)).toEqual([]);
    }).toPass({ timeout: 15_000 });
  });
}

test('the view follows the editor as it is typed into', async ({ page }) => {
  await expect(page.getByTestId('view-empty')).toBeVisible();
  const editor = editorOf(page);
  await editor.click();
  await editor.pressSequentially('# 제목');
  await editor.press('Enter');
  await editor.pressSequentially('한글 **굵게** 그리고 @김');
  await editor.press('Enter');
  await editor.press('Enter');
  await editor.pressSequentially('[ ] 할 일');

  const view = viewOf(page);
  await expect(view.locator('h1')).toHaveText('제목');
  await expect(view.locator('strong')).toHaveText('굵게');
  await expect(view.locator('.mention')).toHaveText('@김하늘');
  await expect(view.getByRole('checkbox', { name: '할 일: 할 일' })).not.toBeChecked();
  await expect(page.getByTestId('view-empty')).toHaveCount(0);

  // Ticking the task in the editor ticks it in the view; the view's own box does not give way.
  await editor.locator('li[data-checked] input').check();
  const box = view.getByRole('checkbox');
  await expect(box).toBeChecked();
  await box.click({ force: true });
  await expect(box).toBeChecked();
  await box.focus();
  await page.keyboard.press('Space');
  await expect(box).toBeChecked();
  await expect(editor.locator('li[data-checked="true"]')).toHaveCount(1);
});

test('both fit a phone 360 pixels wide', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await page.locator('[data-sample="corners"]').click();
  await expect(viewOf(page).locator('h1')).toBeVisible();
  // Nothing sticks out sideways: the page does not scroll horizontally.
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(360);
  for (const body of [editorOf(page), viewOf(page)]) {
    const box = await body.boundingBox();
    expect(box?.x).toBeGreaterThanOrEqual(0);
    expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(360);
  }
  await picturesLoaded(page);
  expect(differences(await measure(page, 'editor'), await measure(page, 'view'))).toEqual([]);

  // Still an editor at this width.
  const editor = editorOf(page);
  await page.locator('[data-sample="empty"]').click();
  await editor.click();
  await editor.pressSequentially('좁은 화면');
  await expect(viewOf(page)).toHaveText('좁은 화면');
});
