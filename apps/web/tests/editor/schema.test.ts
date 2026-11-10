import { describe, expect, it } from 'vitest';

import {
  countTasks,
  emptyDoc,
  isAllowedHref,
  MARK_SPECS,
  MAX_DEPTH,
  NODE_SPECS,
  plainText,
  storedSize,
  toStoredDoc,
  validateDoc,
  type DocNode,
} from '$lib/editor/schema.ts';

const text = (value: string, ...marks: DocNode['marks'] & object): DocNode =>
  marks.length > 0 ? { type: 'text', text: value, marks } : { type: 'text', text: value };
const paragraph = (...content: DocNode[]): DocNode => ({ type: 'paragraph', content });
const doc = (...content: DocNode[]): DocNode => ({ type: 'doc', content });

describe('the list of what a body may contain', () => {
  it('has the fifteen nodes and five marks', () => {
    expect(Object.keys(NODE_SPECS).sort()).toEqual([
      'blockquote',
      'bulletList',
      'codeBlock',
      'doc',
      'hardBreak',
      'heading',
      'horizontalRule',
      'image',
      'listItem',
      'mention',
      'orderedList',
      'paragraph',
      'taskItem',
      'taskList',
      'text',
    ]);
    expect(Object.keys(MARK_SPECS).sort()).toEqual(['bold', 'code', 'italic', 'link', 'strike']);
  });
});

describe('validating a document', () => {
  it('accepts a document that uses every node and mark', () => {
    const everything = doc(
      { type: 'heading', attrs: { level: 2 }, content: [text('Title')] },
      paragraph(
        text('plain '),
        text('bold', { type: 'bold' }),
        text('both', { type: 'italic' }, { type: 'strike' }),
        text('code', { type: 'code' }),
        text('link', { type: 'link', attrs: { href: 'https://example.com/a?b=1' } }),
        { type: 'hardBreak' },
        { type: 'mention', attrs: { userId: 'u1' } },
      ),
      { type: 'bulletList', content: [{ type: 'listItem', content: [paragraph(text('a'))] }] },
      {
        type: 'orderedList',
        attrs: { start: 3 },
        content: [{ type: 'listItem', content: [paragraph(text('c'))] }],
      },
      {
        type: 'taskList',
        content: [
          {
            type: 'taskItem',
            attrs: { checked: true },
            content: [
              paragraph(text('done')),
              {
                type: 'taskList',
                content: [{ type: 'taskItem', attrs: { checked: false }, content: [paragraph()] }],
              },
            ],
          },
        ],
      },
      { type: 'blockquote', content: [paragraph(text('quoted'))] },
      { type: 'codeBlock', attrs: { language: 'rust' }, content: [text('fn main() {}')] },
      { type: 'codeBlock' },
      { type: 'horizontalRule' },
      { type: 'image', attrs: { fileId: 'f1', alt: 'a chart' } },
    );
    expect(validateDoc(everything)).toEqual([]);
    expect(validateDoc(emptyDoc())).toEqual([]);
  });

  it.each<[string, unknown, string]>([
    ['an unknown node', doc({ type: 'table' }), 'unknown node type "table"'],
    [
      'an unknown mark',
      doc(paragraph(text('x', { type: 'underline' }))),
      'unknown mark "underline"',
    ],
    [
      'an unknown attribute',
      doc({ type: 'paragraph', attrs: { textAlign: 'center' } }),
      'unknown attribute "textAlign"',
    ],
    [
      'an unknown member',
      doc({ type: 'paragraph', style: 'x' } as DocNode),
      'unknown member "style"',
    ],
    [
      'a heading level out of range',
      doc({ type: 'heading', attrs: { level: 4 } }),
      '"level" has an invalid value',
    ],
    ['a missing required attribute', doc({ type: 'image' }), '"fileId" is missing'],
    [
      'an image by address',
      doc({ type: 'image', attrs: { fileId: 'f', src: 'https://x/y.png' } }),
      'unknown attribute "src"',
    ],
    [
      'a link with a script',
      doc(paragraph(text('x', { type: 'link', attrs: { href: 'javascript:alert(1)' } }))),
      '"href" has an invalid value',
    ],
    [
      'a link with extra attributes',
      doc(paragraph(text('x', { type: 'link', attrs: { href: '/a', target: '_blank' } }))),
      'unknown attribute "target"',
    ],
    ['empty text', doc(paragraph({ type: 'text', text: '' })), 'text is empty'],
    [
      'a block inside a paragraph',
      doc(paragraph({ type: 'horizontalRule' })),
      'not allowed inside',
    ],
    ['text directly in the document', doc(text('x')), 'not allowed inside'],
    ['a list without entries', doc({ type: 'bulletList' }), 'needs at least 1 child'],
    [
      'a list entry that does not start with a paragraph',
      doc({
        type: 'bulletList',
        content: [{ type: 'listItem', content: [{ type: 'horizontalRule' }] }],
      }),
      'has to start with a paragraph',
    ],
    [
      'marks in a code block',
      doc({ type: 'codeBlock', content: [text('x', { type: 'bold' })] }),
      'no marks',
    ],
    [
      'a mark given twice',
      doc(paragraph(text('x', { type: 'bold' }, { type: 'bold' }))),
      'given twice',
    ],
    ['something that is not a document', { type: 'paragraph' }, 'not allowed inside the root'],
    ['something that is not a node', 'text', 'not a node'],
  ])('refuses %s', (_name, value, problem) => {
    expect(validateDoc(value).join('\n')).toContain(problem);
  });

  it('refuses a document nested deeper than the limit', () => {
    let nested: DocNode = paragraph(text('deep'));
    for (let level = 0; level < MAX_DEPTH; level += 1)
      nested = { type: 'blockquote', content: [nested] };
    expect(validateDoc(doc(nested)).join('\n')).toContain('deeper than');
  });

  it('refuses a document with more nodes than the limit', () => {
    const many = doc(...Array.from({ length: 10_001 }, () => paragraph(text('x'))));
    expect(validateDoc(many).join('\n')).toContain('more than 20000 nodes');
  });
});

describe('link targets', () => {
  it.each([
    'https://example.com',
    'http://example.com/a b',
    'HTTPS://EXAMPLE.COM',
    'mailto:someone@example.com',
    '/board/abc',
    'relative/path?x=1#y',
    '#section',
    '../up',
  ])('allows %s', (href) => {
    expect(isAllowedHref(href)).toBe(true);
  });

  it.each([
    'javascript:alert(1)',
    'JavaScript:alert(1)',
    ' javascript:alert(1)',
    'java\tscript:alert(1)',
    'java\nscript:alert(1)',
    '\u0001javascript:alert(1)',
    'data:text/html,<script>alert(1)</script>',
    'vbscript:msgbox(1)',
    'file:///etc/passwd',
    'ftp://example.com',
    '//example.com/x',
    '\\\\example.com\\x',
    '/\\example.com',
    '',
    '   ',
  ])('refuses %j', (href) => {
    expect(isAllowedHref(href)).toBe(false);
  });

  it('refuses what is not text, and text that is too long', () => {
    expect(isAllowedHref(undefined)).toBe(false);
    expect(isAllowedHref(42)).toBe(false);
    expect(isAllowedHref(`https://example.com/${'a'.repeat(2048)}`)).toBe(false);
  });
});

describe('the stored form', () => {
  it('leaves out attributes without a value and empty attribute lists', () => {
    const fromEditor: DocNode = doc(
      { type: 'codeBlock', attrs: { language: null }, content: [text('x')] },
      { type: 'image', attrs: { fileId: 'f1', alt: null } },
      {
        type: 'orderedList',
        attrs: { start: 1 },
        content: [{ type: 'listItem', content: [paragraph()] }],
      },
    );
    expect(toStoredDoc(fromEditor)).toEqual(
      doc(
        { type: 'codeBlock', content: [text('x')] },
        { type: 'image', attrs: { fileId: 'f1' } },
        {
          type: 'orderedList',
          attrs: { start: 1 },
          content: [{ type: 'listItem', content: [paragraph()] }],
        },
      ),
    );
    expect(validateDoc(fromEditor)).not.toEqual([]);
    expect(validateDoc(toStoredDoc(fromEditor))).toEqual([]);
  });

  it('measures a document in bytes of UTF-8', () => {
    expect(storedSize(doc(paragraph(text('가'))))).toBe(
      '{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":""}]}]}'
        .length + 3,
    );
  });
});

describe('reading a document without an editor', () => {
  const sample = doc(
    { type: 'heading', attrs: { level: 1 }, content: [text('Plan')] },
    paragraph(
      text('ask '),
      { type: 'mention', attrs: { userId: 'u1' } },
      { type: 'hardBreak' },
      text('today'),
    ),
    {
      type: 'taskList',
      content: [
        {
          type: 'taskItem',
          attrs: { checked: true },
          content: [
            paragraph(text('one')),
            {
              type: 'taskList',
              content: [
                {
                  type: 'taskItem',
                  attrs: { checked: false },
                  content: [paragraph(text('inner'))],
                },
              ],
            },
          ],
        },
        { type: 'taskItem', attrs: { checked: true }, content: [paragraph()] },
      ],
    },
    { type: 'image', attrs: { fileId: 'f' } },
  );

  it('counts tasks at every depth', () => {
    expect(countTasks(sample)).toEqual({ total: 3, checked: 2 });
    expect(countTasks(emptyDoc())).toEqual({ total: 0, checked: 0 });
  });

  it('gives the words, one line per paragraph, with mentions by name', () => {
    expect(plainText(sample, (id) => (id === 'u1' ? 'Ada' : id))).toBe(
      'Plan\nask @Ada\ntoday\none\ninner',
    );
    expect(plainText(emptyDoc())).toBe('');
  });
});
