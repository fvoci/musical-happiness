import { describe, expect, it } from 'vitest';

import { addNotes, markdownToBlocks, NOTE_KINDS } from '$lib/editor/markdown.ts';
import { validateDoc, type DocNode } from '$lib/editor/schema.ts';

const blocks = (markdown: string): DocNode[] => markdownToBlocks(markdown).content;
const notes = (markdown: string): Record<string, number> => markdownToBlocks(markdown).notes;
const valid = (content: DocNode[]): string[] => validateDoc({ type: 'doc', content });

describe('Markdown into document nodes', () => {
  it('reads paragraphs, with a single line break as a line break', () => {
    expect(blocks('first line\nsecond line\n\nnext paragraph')).toEqual([
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'first line' },
          { type: 'hardBreak' },
          { type: 'text', text: 'second line' },
        ],
      },
      { type: 'paragraph', content: [{ type: 'text', text: 'next paragraph' }] },
    ]);
  });

  it('reads the inline marks, nested ones too', () => {
    expect(blocks('a **b _c_** ~~d~~ `e`')).toEqual([
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'a ' },
          { type: 'text', text: 'b ', marks: [{ type: 'bold' }] },
          { type: 'text', text: 'c', marks: [{ type: 'bold' }, { type: 'italic' }] },
          { type: 'text', text: ' ' },
          { type: 'text', text: 'd', marks: [{ type: 'strike' }] },
          { type: 'text', text: ' ' },
          { type: 'text', text: 'e', marks: [{ type: 'code' }] },
        ],
      },
    ]);
  });

  it('reads links, written out and bare', () => {
    expect(blocks('[site](https://example.com/a) and https://example.com/b')).toEqual([
      {
        type: 'paragraph',
        content: [
          {
            type: 'text',
            text: 'site',
            marks: [{ type: 'link', attrs: { href: 'https://example.com/a' } }],
          },
          { type: 'text', text: ' and ' },
          {
            type: 'text',
            text: 'https://example.com/b',
            marks: [{ type: 'link', attrs: { href: 'https://example.com/b' } }],
          },
        ],
      },
    ]);
  });

  it('reads headings, lists, task lists, quotes, code and rules', () => {
    const content = blocks(
      [
        '## Title',
        '- a',
        '- b',
        '  - nested',
        '',
        '3. three',
        '4. four',
        '',
        '- [x] done',
        '- [ ] todo',
        '',
        '> quoted',
        '',
        '```js',
        'let x;',
        '```',
        '',
        '---',
      ].join('\n'),
    );
    expect(content.map((node) => node.type)).toEqual([
      'heading',
      'bulletList',
      'orderedList',
      'taskList',
      'blockquote',
      'codeBlock',
      'horizontalRule',
    ]);
    expect(content[0]).toEqual({
      type: 'heading',
      attrs: { level: 2 },
      content: [{ type: 'text', text: 'Title' }],
    });
    expect(content[1]?.content?.[1]?.content?.map((node) => node.type)).toEqual([
      'paragraph',
      'bulletList',
    ]);
    expect(content[2]?.attrs).toEqual({ start: 3 });
    expect(content[3]?.content?.map((item) => item.attrs)).toEqual([
      { checked: true },
      { checked: false },
    ]);
    expect(content[5]).toEqual({
      type: 'codeBlock',
      attrs: { language: 'js' },
      content: [{ type: 'text', text: 'let x;' }],
    });
    expect(valid(content)).toEqual([]);
  });

  it('returns nothing for empty text', () => {
    expect(markdownToBlocks('')).toEqual({ content: [], notes: {} });
    expect(blocks('   \n\n')).toEqual([]);
  });
});

describe('what a document cannot say is counted', () => {
  it('shows HTML as the characters it is, like the original did', () => {
    expect(blocks('a <b>x</b>')).toEqual([
      { type: 'paragraph', content: [{ type: 'text', text: 'a <b>x</b>' }] },
    ]);
    expect(notes('a <b>x</b>')).toEqual({ 'html-as-text': 2 });
    expect(blocks('<div onclick="x()">block</div>')[0]?.content?.[0]?.text).toBe(
      '<div onclick="x()">block</div>',
    );
  });

  it('lowers headings below level three', () => {
    expect(blocks('##### deep')[0]?.attrs).toEqual({ level: 3 });
    expect(notes('##### deep')).toEqual({ 'heading-level-clamped': 1 });
  });

  it('turns a table into a paragraph per row', () => {
    const table = '| a | b |\n|---|---|\n| 1 | **2** |';
    expect(blocks(table)).toEqual([
      { type: 'paragraph', content: [{ type: 'text', text: 'a | b' }] },
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: '1 | ' },
          { type: 'text', text: '2', marks: [{ type: 'bold' }] },
        ],
      },
    ]);
    expect(notes(table)).toEqual({ table: 1 });
  });

  it('turns an image by address into a link', () => {
    expect(blocks('![a chart](https://example.com/c.png)')).toEqual([
      {
        type: 'paragraph',
        content: [
          {
            type: 'text',
            text: 'a chart',
            marks: [{ type: 'link', attrs: { href: 'https://example.com/c.png' } }],
          },
        ],
      },
    ]);
    expect(notes('![a chart](https://example.com/c.png)')).toEqual({ 'image-by-address': 1 });
  });

  it('keeps the text of a link whose target is not allowed', () => {
    expect(blocks('[click](javascript:alert(1))')).toEqual([
      { type: 'paragraph', content: [{ type: 'text', text: 'click' }] },
    ]);
    expect(notes('[click](javascript:alert(1))')).toEqual({ 'link-dropped': 1 });
    expect(notes('[a](https://example.com "tip")')).toEqual({ 'link-title-dropped': 1 });
  });

  it('lets inline code stand alone', () => {
    expect(blocks('**`x`**')).toEqual([
      { type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'code' }] }] },
    ]);
    expect(notes('**`x`**')).toEqual({ 'marks-dropped-in-code': 1 });
  });

  it('writes the boxes out when a list mixes tasks and plain entries', () => {
    const content = blocks('- [x] task\n- plain');
    expect(content[0]?.type).toBe('bulletList');
    expect(content[0]?.content?.[0]?.content?.[0]?.content?.[0]?.text).toBe('[x] task');
    expect(notes('- [x] task\n- plain')).toEqual({ 'mixed-task-list': 1 });
    expect(notes('1. [ ] numbered task')).toEqual({ 'ordered-task-list': 1 });
  });

  it('always produces what the server accepts', () => {
    const awkward = [
      '# h\n\n- \n\n> \n\n```\n```\n\n1. a\n\n   para in item\n\n- [ ] \n',
      '<script>alert(1)</script>\n\n[x](data:text/html,x) ![y](javascript:1)',
      '* a\n\n  ```c++\n  code\n  ```\n\n| t |\n|---|\n',
      '```not a language!\nx\n```',
    ];
    for (const markdown of awkward) expect(valid(blocks(markdown))).toEqual([]);
  });

  it('only uses the documented kinds of note', () => {
    const total: Record<string, number> = {};
    addNotes(total, notes('<b>x</b> ##### h'));
    addNotes(total, notes('| a |\n|---|\n\n<i>y</i>'));
    expect(total).toEqual({ 'html-as-text': 4, table: 1 });
    for (const kind of Object.keys(total)) expect(NOTE_KINDS).toContain(kind);
  });
});
