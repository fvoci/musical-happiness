/**
 * The window under the editor: what is being written survives a change of
 * size (a device unfolded or turned, a window put beside another). The text,
 * the place of the cursor and the focus stay; a list of suggestions that is
 * open stays with the cursor.
 */
import { expect, test, type Page } from '@playwright/test';

interface DocNode {
  type: string;
  content?: DocNode[];
  text?: string;
}

// Rough figures of a folded and an unfolded device, upright and on its side.
const SIZES = [
  { width: 466, height: 678 },
  { width: 626, height: 890 },
  { width: 890, height: 626 },
  { width: 360, height: 740 },
  { width: 1280, height: 800 },
];

const editorOf = (page: Page) => page.getByTestId('editor').locator('.ProseMirror');
const textOf = (node: DocNode): string =>
  (node.text ?? '') + (node.content ?? []).map(textOf).join('');
async function stored(page: Page): Promise<DocNode> {
  return JSON.parse((await page.getByTestId('stored').textContent()) ?? '') as DocNode;
}
/** Where the cursor is: characters from the start of the document's text. */
const cursor = (page: Page): Promise<number> =>
  editorOf(page).evaluate((element) => {
    const selection = getSelection();
    if (!selection || selection.rangeCount === 0) return -1;
    const range = document.createRange();
    range.selectNodeContents(element);
    range.setEnd(selection.focusNode ?? element, selection.focusOffset);
    return range.toString().length;
  });

test.beforeEach(async ({ page }) => {
  await page.goto('/spikes/editor/');
  await editorOf(page).click();
  // Start from an empty body: select what the sample put there and type over it.
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.press('Delete');
});

test('text being written, the cursor and the focus survive every change of size', async ({
  page,
}) => {
  const editor = editorOf(page);
  await editor.pressSequentially('회의록 초안 first line');
  await editor.press('Enter');
  await editor.pressSequentially('second line');
  // The cursor goes into the middle of the second line.
  for (let step = 0; step < 5; step += 1) await page.keyboard.press('ArrowLeft');
  const before = await stored(page);
  const at = await cursor(page);
  expect(at).toBeGreaterThan(0);

  for (const size of SIZES) {
    await page.setViewportSize(size);
    await expect(editor).toBeFocused();
    expect(await stored(page), `${size.width}×${size.height}`).toEqual(before);
    expect(await cursor(page), `${size.width}×${size.height}`).toBe(at);
    // The page does not grow wider than the window.
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
      `${size.width}×${size.height}`,
    ).toBe(true);
  }

  // Typing goes on where the cursor was.
  await page.keyboard.insertText('한');
  expect(textOf(await stored(page))).toBe('회의록 초안 first linesecond한 line');
});

test('a list of suggestions that is open stays open and by the cursor', async ({ page }) => {
  const editor = editorOf(page);
  await editor.pressSequentially('담당 @');
  const list = page.locator('[data-suggestions="mention"]');
  await expect(list).toBeVisible();

  for (const size of SIZES) {
    await page.setViewportSize(size);
    await expect(list, `${size.width}×${size.height}`).toBeVisible();
    // Inside the window, and no further from the cursor's line than its own height.
    const placed = await list.evaluate((element) => {
      const box = element.getBoundingClientRect();
      const range = getSelection()?.getRangeAt(0).getBoundingClientRect();
      return {
        inside: box.left >= 0 && box.right <= window.innerWidth && box.top >= 0,
        gap: range ? Math.abs(box.top - range.bottom) : -1,
        height: box.height,
      };
    });
    expect(placed.inside, `${size.width}×${size.height}`).toBe(true);
    expect(placed.gap, `${size.width}×${size.height}`).toBeLessThanOrEqual(placed.height);
  }

  await page.keyboard.press('Enter');
  await expect(list).toHaveCount(0);
  const doc = await stored(page);
  expect(JSON.stringify(doc)).toContain('"type":"mention"');
});

test('the read-only view follows the editor in every size', async ({ page }) => {
  const editor = editorOf(page);
  await editor.pressSequentially('한 줄');
  for (const size of SIZES) {
    await page.setViewportSize(size);
    await expect(page.getByTestId('view')).toContainText('한 줄');
  }
});
