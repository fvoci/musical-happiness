import { expect, test, type Page } from '@playwright/test';

interface DocNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: DocNode[];
  marks?: { type: string; attrs?: Record<string, unknown> }[];
  text?: string;
}

const editorOf = (page: Page) => page.getByTestId('editor').locator('.ProseMirror');

/** What would be stored now. An empty body is stored as nothing; here that reads as an empty document. */
async function stored(page: Page): Promise<DocNode> {
  const shown = JSON.parse(
    (await page.getByTestId('stored').textContent()) ?? '',
  ) as DocNode | null;
  return shown ?? { type: 'doc', content: [{ type: 'paragraph' }] };
}

const types = (node: DocNode): string[] => [node.type, ...(node.content ?? []).flatMap(types)];
const textOf = (node: DocNode): string =>
  (node.text ?? '') + (node.content ?? []).map(textOf).join('');

/** Pastes as a browser does when the clipboard holds HTML (and a plain-text fallback). */
async function paste(page: Page, html: string, plain = ''): Promise<void> {
  await editorOf(page).evaluate(
    (element, [markup, text]) => {
      const data = new DataTransfer();
      data.setData('text/html', markup ?? '');
      data.setData('text/plain', text ?? '');
      element.dispatchEvent(
        new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
      );
    },
    [html, plain],
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto('/spikes/editor/');
  await editorOf(page).click();
});

test('typing with shortcuts produces a document the server accepts', async ({ page }) => {
  const editor = editorOf(page);
  await editor.pressSequentially('# Title');
  await editor.press('Enter');
  await editor.pressSequentially('plain **bold** and `code`');
  await editor.press('Enter');
  await editor.pressSequentially('- one');
  await editor.press('Enter');
  await editor.pressSequentially('two');
  await editor.press('Enter');
  await editor.press('Enter');
  await editor.pressSequentially('[ ] a task');

  const doc = await stored(page);
  expect(doc.content?.map((node) => node.type)).toEqual([
    'heading',
    'paragraph',
    'bulletList',
    'taskList',
  ]);
  expect(doc.content?.[0]).toEqual({
    type: 'heading',
    attrs: { level: 1 },
    content: [{ type: 'text', text: 'Title' }],
  });
  expect(doc.content?.[1]?.content).toEqual([
    { type: 'text', text: 'plain ' },
    { type: 'text', text: 'bold', marks: [{ type: 'bold' }] },
    { type: 'text', text: ' and ' },
    { type: 'text', text: 'code', marks: [{ type: 'code' }] },
  ]);
  expect(doc.content?.[3]?.content?.[0]?.attrs).toEqual({ checked: false });
  await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
});

test('the toolbar toggles marks and blocks', async ({ page }) => {
  const editor = editorOf(page);
  await page.getByRole('button', { name: '굵게' }).click();
  await editor.pressSequentially('strong');
  await expect(page.getByRole('button', { name: '굵게' })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: '인용', exact: true }).click();
  const doc = await stored(page);
  expect(doc.content?.[0]?.type).toBe('blockquote');
  expect(doc.content?.[0]?.content?.[0]?.content?.[0]?.marks).toEqual([{ type: 'bold' }]);
});

test('@ offers people and inserts a mention that holds only the user identifier', async ({
  page,
}) => {
  const editor = editorOf(page);
  await editor.pressSequentially('담당 @김');
  const list = page.locator('[data-suggestions="mention"]');
  await expect(list.getByRole('option')).toHaveText(['김하늘']);
  await editor.press('Enter');
  await expect(list).toHaveCount(0);

  const doc = await stored(page);
  expect(doc.content?.[0]?.content).toEqual([
    { type: 'text', text: '담당 ' },
    { type: 'mention', attrs: { userId: '01930000-0000-7000-8000-00000000c001' } },
    { type: 'text', text: ' ' },
  ]);
  await expect(editor.locator('[data-mention]')).toHaveText('@김하늘');
});

test('arrow keys move through the list of people, Escape closes it', async ({ page }) => {
  const editor = editorOf(page);
  await editor.pressSequentially('@');
  const options = page.locator('[data-suggestions="mention"]').getByRole('option');
  await expect(options).toHaveCount(4);
  await editor.press('ArrowDown');
  await expect(options.nth(1)).toHaveAttribute('aria-selected', 'true');
  await editor.press('Escape');
  await expect(page.locator('[data-suggestions="mention"]')).toHaveCount(0);
  expect(types(await stored(page))).not.toContain('mention');
});

test(': offers emoji and inserts the character as text', async ({ page }) => {
  const editor = editorOf(page);
  await editor.pressSequentially('좋아요 :rocket');
  // The real list now, from CLDR: the emoji and its name.
  await expect(page.locator('[data-suggestions="emoji"]').getByRole('option').first()).toHaveText(
    '🚀 rocket',
  );
  await editor.press('Enter');
  expect(textOf(await stored(page))).toBe('좋아요 🚀');

  // A colon inside a time or an address does not open the list.
  await editor.pressSequentially(' 10:30');
  await expect(page.locator('[data-suggestions="emoji"]')).toHaveCount(0);
});

test('an image is stored by file identifier and drawn from the resolved address', async ({
  page,
}) => {
  await page.getByRole('button', { name: '이미지' }).click();
  const doc = await stored(page);
  expect(doc.content?.find((node) => node.type === 'image')).toEqual({
    type: 'image',
    attrs: { fileId: 'file-1' },
  });
  const image = editorOf(page).locator('img');
  await expect(image).toHaveAttribute('data-file-id', 'file-1');
  await expect(image).toHaveAttribute('src', /^data:image\/svg\+xml/);
  expect(JSON.stringify(doc)).not.toContain('data:image');
  // The picture really loads.
  expect(await image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBe(480);
});

test('pasted content outside the list is dropped; the text stays', async ({ page }) => {
  await paste(
    page,
    '<h1 style="color:red" onclick="steal()">Heading</h1>' +
      '<p>para <u>under</u> <span style="font-size:40px">big</span> <a href="javascript:alert(1)">bad link</a> ' +
      '<a href="https://example.com/ok" target="_top" onclick="x()">good link</a></p>' +
      '<script>window.pwned = true</script><iframe src="https://evil.example/"></iframe>' +
      '<img src="https://tracker.example/pixel.gif" alt="pixel">' +
      '<table><tr><td>cell</td></tr></table>' +
      '<ul><li>item</li></ul><h5>small heading</h5>',
  );
  const doc = await stored(page);
  await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');

  const json = JSON.stringify(doc);
  for (const forbidden of [
    'script',
    'pwned',
    'iframe',
    'evil',
    'tracker',
    'steal',
    'color',
    'font-size',
    'javascript',
    '_top',
    'onclick',
    'table',
  ]) {
    expect(json).not.toContain(forbidden);
  }
  expect(types(doc)).not.toContain('image');
  const text = textOf(doc);
  for (const kept of [
    'Heading',
    'para',
    'under',
    'big',
    'bad link',
    'good link',
    'cell',
    'item',
    'small heading',
  ]) {
    expect(text).toContain(kept);
  }
  const links = (doc.content ?? [])
    .flatMap((node) => node.content ?? [])
    .flatMap((node) => node.marks ?? []);
  expect(links).toEqual([{ type: 'link', attrs: { href: 'https://example.com/ok' } }]);
  expect(await page.evaluate(() => (window as { pwned?: boolean }).pwned)).toBeUndefined();
});

test('pasting a bare address makes a link; a script address stays text', async ({ page }) => {
  await paste(page, '', 'https://example.com/pasted');
  await editorOf(page).press('Enter');
  await paste(page, '', 'javascript:alert(1)');
  const doc = await stored(page);
  expect(doc.content?.[0]?.content?.[0]?.marks).toEqual([
    { type: 'link', attrs: { href: 'https://example.com/pasted' } },
  ]);
  expect(doc.content?.[1]?.content).toEqual([{ type: 'text', text: 'javascript:alert(1)' }]);
});

test('the sample that uses every node and mark loads unchanged and is valid', async ({ page }) => {
  await page.locator('[data-sample="everything"]').click();
  const doc = await stored(page);
  await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
  expect([...new Set(types(doc))].sort()).toEqual([
    'blockMath',
    'blockquote',
    'bulletList',
    'codeBlock',
    'doc',
    'hardBreak',
    'heading',
    'horizontalRule',
    'image',
    'inlineMath',
    'listItem',
    'mention',
    'orderedList',
    'paragraph',
    'taskItem',
    'taskList',
    'text',
  ]);
  // Ticking a task changes the stored document.
  await editorOf(page).locator('li[data-checked="false"] input').first().check();
  const after = await stored(page);
  const tasks = after.content?.find((node) => node.type === 'taskList')?.content ?? [];
  expect(tasks.map((task) => task.attrs?.checked)).toEqual([true, true, false]);
});

test('Markdown is brought in as a document', async ({ page }) => {
  await page.getByText('마크다운 가져오기').click();
  await page
    .getByLabel('마크다운')
    .fill('## From Markdown\n\n- [x] done\n- [ ] todo\n\n| a | b |\n|---|---|\n| 1 | 2 |');
  await page.getByTestId('import-markdown').click();
  const doc = await stored(page);
  expect(doc.content?.map((node) => node.type)).toEqual([
    'heading',
    'taskList',
    'paragraph',
    'paragraph',
  ]);
  await expect(page.getByTestId('status')).toHaveAttribute('data-valid', 'true');
});
