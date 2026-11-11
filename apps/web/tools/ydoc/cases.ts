/**
 * Shared content written by hand, for `contracts/ydoc-vectors.json`: what no
 * editor writes but a merge of two edits, an older binding or a client that
 * is not ours may leave in a Y document.
 *
 * `READINGS` are kept by the editors as they are, and read in a way the
 * shared content alone does not tell. `REPAIRS` do not fit the editor's
 * schema: every editor that sees one deletes what fails, and what the vectors
 * hold is what is left. Each case is the children of the root fragment.
 */
import type { RawNode } from './shape.ts';

export interface RawCase {
  name: string;
  root: RawNode[];
}

const ADA = '01930000-0000-7000-8000-00000000c002';

const text = (insert: string, attributes?: Record<string, unknown>): RawNode => ({
  text: [attributes ? { insert, attributes } : { insert }],
});
const paragraph = (...children: RawNode[]): RawNode => ({ element: 'paragraph', children });
const listItem = (...children: RawNode[]): RawNode => ({ element: 'listItem', children });
const bulletList = (...children: RawNode[]): RawNode => ({ element: 'bulletList', children });
const blockquote = (...children: RawNode[]): RawNode => ({ element: 'blockquote', children });

export const READINGS: RawCase[] = [
  {
    name: 'a heading without a level is of level 1',
    root: [{ element: 'heading', children: [text('h')] }],
  },
  {
    name: 'a task item without checked is not checked',
    root: [{ element: 'taskList', children: [{ element: 'taskItem', children: [paragraph()] }] }],
  },
  {
    name: 'an ordered list without a start starts at 1',
    root: [{ element: 'orderedList', children: [listItem(paragraph(text('n')))] }],
  },
  {
    name: 'an attribute nobody knows is not read',
    root: [
      { element: 'paragraph', attrs: { colour: 'red', level: 2 }, children: [text('p')] },
      { element: 'image', attrs: { fileId: 'f.png', width: 10 } },
    ],
  },
  {
    name: 'texts side by side are one text',
    root: [paragraph(text('a'), text('b'), text('c', { bold: {} }), text('d', { bold: {} }))],
  },
  {
    name: 'an empty text is nothing',
    root: [paragraph(text('a')), paragraph({ text: [] }), paragraph({ text: [] }, text('b'))],
  },
  {
    name: 'a document of one paragraph with an empty text is no body',
    root: [paragraph({ text: [] })],
  },
  {
    name: 'an empty text between two elements does not part them',
    root: [paragraph({ element: 'hardBreak' }, { text: [] }, { element: 'hardBreak' })],
  },
  {
    name: 'marks come in the order of the schema, however they were written',
    root: [
      paragraph({
        text: [
          {
            insert: 'x',
            attributes: {
              code: {},
              strike: {},
              italic: {},
              bold: {},
              link: { href: 'https://example.com/' },
            },
          },
        ],
      }),
    ],
  },
  {
    name: 'bold and code together, which the editor never sets, are both kept',
    root: [paragraph(text('x', { bold: {}, code: {} }))],
  },
  {
    name: 'a mark named with a hash is the mark before the hash',
    root: [paragraph(text('x', { 'bold--abcdefgh': {} }), text('y', { 'italic--AB+/=012': {} }))],
  },
  {
    name: 'a mark whose value is true is the mark',
    root: [paragraph(text('x', { bold: true }))],
  },
  {
    name: 'an attribute of a mark nobody knows is not read',
    root: [
      paragraph(text('x', { link: { href: '/boards', target: '_blank' }, bold: { weight: 9 } })),
    ],
  },
  {
    name: 'runs with the same marks after reading are one text',
    root: [paragraph(text('a', { bold: {} }), text('b', { 'bold--abcdefgh': {} }))],
  },
];

export const REPAIRS: RawCase[] = [
  { name: 'a bullet list without entries', root: [paragraph(text('a')), bulletList()] },
  {
    name: 'an ordered list and a task list without entries',
    root: [
      { element: 'orderedList', attrs: { start: 3 } },
      paragraph(text('a')),
      { element: 'taskList' },
    ],
  },
  { name: 'a blockquote with nothing in it', root: [blockquote(), paragraph(text('a'))] },
  {
    name: 'a list entry with nothing in it, beside one that is whole',
    root: [bulletList(listItem(), listItem(paragraph(text('kept'))))],
  },
  {
    name: 'a list whose only entry has nothing in it: the entry, then the list',
    root: [paragraph(text('a')), bulletList(listItem())],
  },
  {
    name: 'a list entry that does not start with a paragraph',
    root: [
      bulletList(listItem(blockquote(paragraph(text('q')))), listItem(paragraph(text('kept')))),
    ],
  },
  {
    name: 'a task item that starts with a list',
    root: [
      paragraph(text('a')),
      {
        element: 'taskList',
        children: [
          {
            element: 'taskItem',
            attrs: { checked: true },
            children: [bulletList(listItem(paragraph()))],
          },
        ],
      },
    ],
  },
  {
    name: 'what fails deep down takes its emptied ancestors with it',
    root: [paragraph(text('a')), blockquote(bulletList(listItem(bulletList())))],
  },
  {
    name: 'a failing node between whole ones leaves them',
    root: [blockquote(paragraph(text('a')), bulletList(), paragraph(text('b')))],
  },
  {
    name: 'an element of a kind nobody knows',
    root: [paragraph(text('a')), { element: 'video', attrs: { src: 'x' } }],
  },
  {
    name: 'an element of a kind nobody knows in a line: it goes, the line stays',
    root: [paragraph(text('a'), { element: 'emoji', attrs: { name: 'x' } }, text('b'))],
  },
  {
    name: 'a text with a mark nobody knows goes whole, its neighbours stay',
    root: [
      paragraph(
        text('a'),
        { element: 'hardBreak' },
        { text: [{ insert: 'b' }, { insert: 'glow', attributes: { glow: {} } }] },
      ),
    ],
  },
  {
    name: 'a code block whose text has a mark',
    root: [paragraph(text('a')), { element: 'codeBlock', children: [text('b', { bold: {} })] }],
  },
  {
    name: 'a code block that holds an element',
    root: [
      paragraph(text('a')),
      { element: 'codeBlock', children: [text('b'), { element: 'hardBreak' }] },
    ],
  },
  {
    name: 'a paragraph that holds a block: the paragraph goes, not the block alone',
    root: [paragraph(text('a')), paragraph(text('b'), paragraph(text('c')))],
  },
  {
    name: 'a heading that holds a list',
    root: [
      paragraph(text('a')),
      { element: 'heading', attrs: { level: 2 }, children: [bulletList(listItem(paragraph()))] },
    ],
  },
  {
    name: 'leaves with something inside',
    root: [
      paragraph(text('a'), { element: 'hardBreak', children: [text('x')] }),
      { element: 'image', attrs: { fileId: 'f.png' }, children: [paragraph()] },
      { element: 'horizontalRule', children: [paragraph()] },
      { element: 'blockMath', attrs: { latex: 'x' }, children: [text('x')] },
      paragraph({ element: 'mention', attrs: { userId: ADA }, children: [text('x')] }),
      paragraph({ element: 'inlineMath', attrs: { latex: 'x' }, children: [text('x')] }, text('b')),
    ],
  },
  {
    name: 'a blockquote that holds a text',
    root: [paragraph(text('a')), blockquote(text('loose'))],
  },
  {
    name: 'a list that holds an entry of another kind of list',
    root: [
      paragraph(text('a')),
      bulletList({ element: 'taskItem', attrs: { checked: false }, children: [paragraph()] }),
      { element: 'taskList', children: [listItem(paragraph())] },
    ],
  },
  {
    name: 'a list that holds a paragraph beside an entry',
    root: [paragraph(text('a')), bulletList(listItem(paragraph(text('b'))), paragraph(text('c')))],
  },
  {
    name: 'a list entry inside a blockquote',
    root: [paragraph(text('a')), blockquote(listItem(paragraph(text('b'))))],
  },
  { name: 'nothing is left', root: [bulletList(), { element: 'video' }, blockquote(blockquote())] },
];
