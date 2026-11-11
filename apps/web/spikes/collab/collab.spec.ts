/**
 * What collaboration on a body rests on, held by running it: the shape of the
 * shared document, how a change from elsewhere enters an editor, and what an
 * editor does with shared content its schema has no place for. The server's
 * conversion (`crates/collab`) is written against these.
 */
import { expect, test, type Page } from '@playwright/test';

import type { DocNode } from '../../src/lib/editor/schema.ts';
import type { RawNode, RawRun, Shape } from '../../tools/ydoc/shape.ts';

import './api.ts';

const ADA = '01930000-0000-7000-8000-00000000c002';

const text = (value: string, ...marks: NonNullable<DocNode['marks']>): DocNode =>
  marks.length > 0 ? { type: 'text', text: value, marks } : { type: 'text', text: value };
const paragraph = (...content: DocNode[]): DocNode =>
  content.length > 0 ? { type: 'paragraph', content } : { type: 'paragraph' };
const item = (...content: DocNode[]): DocNode => ({ type: 'listItem', content });
const doc = (...content: DocNode[]): DocNode => ({ type: 'doc', content });

/** An element as `shapeOf` writes it out. */
const element = (
  name: string,
  attrs: Record<string, unknown> = {},
  children: Shape[] = [],
): Shape => ({
  kind: 'XmlElement',
  name,
  attrs: Object.fromEntries(
    Object.entries(attrs).map(([key, value]) => [key, { type: typeof value, value }]),
  ),
  children,
});
const run = (insert: string, attributes?: Record<string, unknown>): RawRun =>
  attributes ? { insert, attributes } : { insert };
const runs = (...content: RawRun[]): Shape => ({ kind: 'XmlText', runs: content });
const fragment = (...children: Shape[]): Shape => ({ kind: 'XmlFragment', children });

/** Three blocks: a paragraph, a list of two entries, a paragraph. */
const THREE = doc(
  paragraph(text('first')),
  { type: 'bulletList', content: [item(paragraph(text('one'))), item(paragraph(text('two')))] },
  paragraph(text('last')),
);
const THREE_SHAPE: [Shape, Shape, Shape] = [
  element('paragraph', {}, [runs(run('first'))]),
  element('bulletList', {}, [
    element('listItem', {}, [element('paragraph', {}, [runs(run('one'))])]),
    element('listItem', {}, [element('paragraph', {}, [runs(run('two'))])]),
  ]),
  element('paragraph', {}, [runs(run('last'))]),
];

/** Opens the page; gives the warnings and errors of its console as they come. */
async function open(page: Page, query = ''): Promise<string[]> {
  const said: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'warning' || message.type() === 'error') said.push(message.text());
  });
  page.on('pageerror', (error) => said.push(`uncaught: ${error.message}`));
  await page.goto(`/spikes/collab/${query}`);
  await page.locator('body[data-ready]').waitFor();
  return said;
}

const seed = (page: Page, content: DocNode): Promise<void> =>
  page.evaluate((given) => {
    window.spike.seed(given);
  }, content);
const stored = (page: Page, peer: 'a' | 'b'): Promise<DocNode | null> =>
  page.evaluate((name) => window.spike.stored(name), peer);
const shape = (page: Page, peer: 'a' | 'b'): Promise<Shape> =>
  page.evaluate((name) => window.spike.shape(name), peer);
const inject = (page: Page, path: number[], index: number, nodes: RawNode[]): Promise<void> =>
  page.evaluate(
    ([where, at, given]) => {
      window.spike.inject(where, at, given);
    },
    [path, index, nodes] as const,
  );

/** Both sides hold the same shared content and show the same document. */
async function expectSame(page: Page): Promise<void> {
  expect(await shape(page, 'b')).toEqual(await shape(page, 'a'));
  expect(await stored(page, 'b')).toEqual(await stored(page, 'a'));
}

test.describe('the shape of the shared document', () => {
  test('is one XML fragment named "default", empty until content arrives', async ({ page }) => {
    await open(page);
    const before = await page.evaluate(() => ({
      field: window.spike.field,
      roots: window.spike.roots('a'),
      shape: window.spike.shape('a'),
      shown: window.spike.json('a'),
    }));
    // The editors were told no name: this is the one the extension has for it.
    expect(before.field).toBe('default');
    expect(before.roots).toEqual([{ name: 'default', kind: 'YXmlFragment' }]);
    // The editor shows its empty paragraph and writes nothing of it into the fragment.
    expect(before.shape).toEqual(fragment());
    expect(before.shown).toEqual(doc(paragraph()));

    await seed(page, THREE);
    expect(await page.evaluate(() => window.spike.roots('a'))).toEqual(before.roots);
    expect(await shape(page, 'a')).toEqual(fragment(...THREE_SHAPE));
    expect(await stored(page, 'a')).toEqual(THREE);
    await expectSame(page);
  });

  test('has an element per node, named for its type, with its attributes as they are', async ({
    page,
  }) => {
    await open(page);
    await seed(
      page,
      doc(
        { type: 'heading', attrs: { level: 2 }, content: [text('Title')] },
        paragraph(
          text('a'),
          { type: 'hardBreak' },
          { type: 'mention', attrs: { userId: ADA } },
          { type: 'inlineMath', attrs: { latex: 'x^2' } },
          text('b'),
        ),
        { type: 'orderedList', content: [item(paragraph(text('n')))] },
        { type: 'orderedList', attrs: { start: 7 }, content: [item(paragraph())] },
        {
          type: 'taskList',
          content: [{ type: 'taskItem', attrs: { checked: true }, content: [paragraph()] }],
        },
        { type: 'blockquote', content: [paragraph(text('q'))] },
        { type: 'codeBlock', attrs: { language: 'mermaid' }, content: [text('graph TD\n  A-->B')] },
        { type: 'codeBlock', content: [text('x')] },
        { type: 'image', attrs: { fileId: 'file-1.png' } },
        { type: 'image', attrs: { fileId: 'file-2.png', alt: '' } },
        { type: 'blockMath', attrs: { latex: 'E=mc^2' } },
        { type: 'horizontalRule' },
        paragraph(),
      ),
    );
    expect(await shape(page, 'a')).toEqual(
      fragment(
        // A number stays a number, a boolean a boolean.
        element('heading', { level: 2 }, [runs(run('Title'))]),
        // Text is a text type; what stands in a line and is no text is an element between two.
        element('paragraph', {}, [
          runs(run('a')),
          element('hardBreak'),
          element('mention', { userId: ADA }),
          element('inlineMath', { latex: 'x^2' }),
          runs(run('b')),
        ]),
        // An attribute left out is written with the value the editor gives it,
        element('orderedList', { start: 1 }, [
          element('listItem', {}, [element('paragraph', {}, [runs(run('n'))])]),
        ]),
        // a paragraph made empty has nothing inside,
        element('orderedList', { start: 7 }, [element('listItem', {}, [element('paragraph')])]),
        element('taskList', {}, [element('taskItem', { checked: true }, [element('paragraph')])]),
        element('blockquote', {}, [element('paragraph', {}, [runs(run('q'))])]),
        element('codeBlock', { language: 'mermaid' }, [runs(run('graph TD\n  A-->B'))]),
        // and an attribute without a value is not written at all.
        element('codeBlock', {}, [runs(run('x'))]),
        element('image', { fileId: 'file-1.png' }),
        element('image', { fileId: 'file-2.png', alt: '' }),
        element('blockMath', { latex: 'E=mc^2' }),
        element('horizontalRule'),
        element('paragraph'),
      ),
    );
    await expectSame(page);
  });

  test('keeps marks as the formatting of a text, one member per mark', async ({ page }) => {
    await open(page);
    const link = { type: 'link', attrs: { href: 'https://example.com/' } };
    await seed(
      page,
      doc(
        paragraph(
          text('plain '),
          text('bold', { type: 'bold' }),
          // Written against the order of the schema, to see which order comes back.
          text('both', { type: 'italic' }, { type: 'bold' }),
          text('all', { type: 'code' }, { type: 'strike' }, link),
        ),
      ),
    );
    // Neighbouring texts are one text type, whatever their marks. A mark is a
    // member named for it whose value is its attributes, in the schema's order.
    expect(await shape(page, 'a')).toEqual(
      fragment(
        element('paragraph', {}, [
          runs(
            run('plain '),
            run('bold', { bold: {} }),
            run('both', { bold: {}, italic: {} }),
            run('all', { link: { href: 'https://example.com/' }, strike: {}, code: {} }),
          ),
        ]),
      ),
    );
    // The members of a text's formatting are written in that order too.
    const written = (await shape(page, 'a')) as { children: { children: Shape[] }[] };
    const formatted = written.children[0]?.children[0];
    expect(
      formatted?.kind === 'XmlText'
        ? formatted.runs.map((one) => Object.keys(one.attributes ?? {}))
        : null,
    ).toEqual([[], ['bold'], ['bold', 'italic'], ['link', 'strike', 'code']]);
    expect(await page.evaluate(() => window.spike.markOrder())).toEqual([
      'link',
      'bold',
      'italic',
      'strike',
      'code',
    ]);
    // What the editor stores has the marks in that order, however they were written.
    expect((await stored(page, 'a'))?.content?.[0]?.content?.slice(2)).toEqual([
      text('both', { type: 'bold' }, { type: 'italic' }),
      text('all', link, { type: 'strike' }, { type: 'code' }),
    ]);
  });

  test('has no two marks of a kind on a text: a link over a link takes its place', async ({
    page,
  }) => {
    await open(page);
    await seed(
      page,
      doc(paragraph(text('abcd', { type: 'link', attrs: { href: 'https://one.example/' } }))),
    );
    await page.evaluate(() => {
      window.spike.b.editor
        .chain()
        .setTextSelection({ from: 2, to: 4 })
        .setMark('link', { href: 'https://two.example/' })
        .run();
    });
    // No mark of the schema may lie over another of its kind, so no member's
    // name carries the hash the binding gives to marks that may.
    expect(await shape(page, 'a')).toEqual(
      fragment(
        element('paragraph', {}, [
          runs(
            run('a', { link: { href: 'https://one.example/' } }),
            run('bc', { link: { href: 'https://two.example/' } }),
            run('d', { link: { href: 'https://one.example/' } }),
          ),
        ]),
      ),
    );
    await expectSame(page);
  });

  test('keeps an empty text in a paragraph an editor emptied', async ({ page }) => {
    await open(page);
    await seed(page, THREE);
    await page.evaluate(() => {
      const { spike } = window;
      const first = spike.range('a', 'paragraph', 0);
      spike.a.editor.commands.deleteRange({ from: first.from + 1, to: first.to - 1 });
    });
    // Not the paragraph without anything inside that one made empty has.
    expect(((await shape(page, 'a')) as { children: Shape[] }).children[0]).toEqual(
      element('paragraph', {}, [runs()]),
    );
    expect((await stored(page, 'a'))?.content?.[0]).toEqual(paragraph());
    await expectSame(page);
  });
});

test.describe('a change from elsewhere', () => {
  test('replaces the top-level blocks that changed and no other', async ({ page }) => {
    await open(page);
    await seed(page, THREE);
    // What first arrives takes the place of the whole document.
    expect(await page.evaluate(() => window.spike.changes('b').at(-1))).toEqual({
      remote: true,
      steps: [{ kind: 'replace', from: 0, to: 2, inserted: 29 }],
      before: 2,
      after: 29,
    });

    const seen = await page.evaluate(() => {
      const { spike } = window;
      spike.clearLogs();
      const first = spike.range('a', 'paragraph', 0);
      spike.a.editor.commands.insertContentAt(first.from + 1, 'XX');
      const one = [...spike.changes('b')];
      spike.clearLogs();
      // One change of A's to the first block and the last.
      const size = spike.a.editor.state.doc.content.size;
      spike.a.editor
        .chain()
        .insertContentAt(size - 1, 'Z')
        .insertContentAt(1, 'Y')
        .run();
      return { first, one, both: spike.changes('b'), size };
    });
    // One step, over the first paragraph alone: from its start to its end.
    expect(seen.first).toEqual({ from: 0, to: 7 });
    expect(seen.one).toEqual([
      {
        remote: true,
        steps: [{ kind: 'replace', from: 0, to: 7, inserted: 9 }],
        before: 29,
        after: 31,
      },
    ]);
    // From the first block that changed to the last, with the list between them.
    expect(seen.size).toBe(31);
    expect(seen.both).toEqual([
      {
        remote: true,
        steps: [{ kind: 'replace', from: 0, to: 31, inserted: 33 }],
        before: 31,
        after: 33,
      },
    ]);
  });

  test('keeps a place in another block, and loses a place in a block it changed', async ({
    page,
  }) => {
    await open(page);
    await seed(page, THREE);
    const seen = await page.evaluate(() => {
      const { spike } = window;
      // Inside "two", the second entry of the list.
      const place = spike.range('b', 'listItem', 1).from + 3;
      spike.placeMarker('b', place);
      const placed = spike.marker('b');
      spike.a.editor.commands.insertContentAt(1, 'XX');
      const afterAnotherBlock = spike.marker('b');
      // Now a change in the first entry of the same list.
      const list = spike.range('b', 'bulletList', 0);
      spike.a.editor.commands.insertContentAt(spike.range('a', 'listItem', 0).from + 3, 'Y');
      return { place, placed, afterAnotherBlock, list, afterSameBlock: spike.marker('b') };
    });
    expect(seen.placed).toEqual({ position: seen.place, deleted: false, decoration: seen.place });
    expect(seen.afterAnotherBlock).toEqual({
      position: seen.place + 2,
      deleted: false,
      decoration: seen.place + 2,
    });
    // The whole list was replaced: the position is pushed to the end of what
    // took its place and is told it was deleted, and the decoration is gone.
    expect(seen.afterSameBlock).toEqual({
      position: seen.list.to + 1,
      deleted: true,
      decoration: null,
    });
  });
});

test.describe('shared content the schema has no place for', () => {
  interface Case {
    name: string;
    /** What is put in: at the root after the first block, or inside the block at `path`. */
    nodes: RawNode[];
    path?: number[];
    /** How many deletions each editor writes; none when it keeps what was put in. */
    deletions: number;
    /** What is left at the root in place of what was put in. */
    left?: Shape[];
  }
  const cases: Case[] = [
    { name: 'a list without entries', nodes: [{ element: 'bulletList' }], deletions: 1 },
    {
      name: 'an element of a kind nobody knows',
      nodes: [{ element: 'video', attrs: { src: 'x' } }],
      deletions: 1,
    },
    {
      name: 'a text with a mark nobody knows',
      path: [0],
      nodes: [{ text: [{ insert: 'glow', attributes: { glow: {} } }] }],
      deletions: 1,
    },
    {
      name: 'a code block whose text has a mark',
      nodes: [
        { element: 'codeBlock', children: [{ text: [{ insert: 'b', attributes: { bold: {} } }] }] },
      ],
      deletions: 1,
    },
    {
      name: 'a node that holds what it may not: all of it goes, not the child alone',
      nodes: [{ element: 'paragraph', children: [{ element: 'paragraph' }] }],
      deletions: 1,
    },
    {
      name: 'a leaf with something inside',
      nodes: [{ element: 'image', attrs: { fileId: 'f' }, children: [{ element: 'paragraph' }] }],
      deletions: 1,
    },
    {
      name: 'a list whose only entry does not start with a paragraph: the entry, then the list',
      nodes: [
        {
          element: 'bulletList',
          children: [
            {
              element: 'listItem',
              children: [{ element: 'blockquote', children: [{ element: 'paragraph' }] }],
            },
          ],
        },
      ],
      deletions: 2,
    },
    {
      name: 'a list without entries inside a quote: the quote stays, with what else it holds',
      nodes: [
        {
          element: 'blockquote',
          children: [
            { element: 'paragraph', children: [{ text: [{ insert: 'q' }] }] },
            { element: 'bulletList' },
            { element: 'paragraph' },
          ],
        },
      ],
      deletions: 1,
      left: [
        element('blockquote', {}, [
          element('paragraph', {}, [runs(run('q'))]),
          element('paragraph'),
        ]),
      ],
    },
    {
      name: 'a heading of a level the server refuses: the schema checks no value',
      nodes: [{ element: 'heading', attrs: { level: 9 }, children: [{ text: [{ insert: 'h' }] }] }],
      deletions: 0,
      left: [element('heading', { level: 9 }, [runs(run('h'))])],
    },
    {
      name: 'a link that is a text, not attributes',
      nodes: [
        {
          element: 'paragraph',
          children: [{ text: [{ insert: 'x', attributes: { link: 'no' } }] }],
        },
      ],
      deletions: 0,
      left: [element('paragraph', {}, [runs(run('x', { link: 'no' }))])],
    },
  ];

  for (const one of cases) {
    const verdict = one.deletions > 0 ? 'deleted by each editor on its own' : 'kept';
    test(`${one.name} is ${verdict}`, async ({ page }) => {
      const said = await open(page);
      await seed(page, THREE);
      const wrote = await page.evaluate(
        ([where, given]) => {
          const { spike } = window;
          spike.clearLogs();
          spike.inject(where, 1, given);
          return { a: spike.writes('a'), b: spike.writes('b'), thrown: spike.thrown };
        },
        [one.path ?? [], one.nodes] as const,
      );
      // Both were handed it before either's answer reached the other.
      expect(wrote).toEqual({ a: one.deletions, b: one.deletions, thrown: [] });
      expect(await shape(page, 'a')).toEqual(
        fragment(THREE_SHAPE[0], ...(one.left ?? []), THREE_SHAPE[1], THREE_SHAPE[2]),
      );
      if (!one.left) expect(await stored(page, 'a')).toEqual(THREE);
      await expectSame(page);
      expect(said).toEqual([]);
    });
  }

  test('an attribute left out is read as the editor gives it, and one nobody knows is not read', async ({
    page,
  }) => {
    await open(page);
    await seed(page, THREE);
    await inject(page, [], 0, [
      { element: 'heading', attrs: { colour: 'red' }, children: [{ text: [{ insert: 'h' }] }] },
      {
        element: 'taskList',
        children: [{ element: 'taskItem', children: [{ element: 'paragraph' }] }],
      },
    ]);
    // The shared content stays as it was written,
    const after = (await shape(page, 'a')) as { children: Shape[] };
    expect(after.children.slice(0, 2)).toEqual([
      element('heading', { colour: 'red' }, [runs(run('h'))]),
      element('taskList', {}, [element('taskItem', {}, [element('paragraph')])]),
    ]);
    // and the editors read it with the schema's own values.
    expect((await stored(page, 'a'))?.content?.slice(0, 2)).toEqual([
      { type: 'heading', attrs: { level: 1 }, content: [text('h')] },
      {
        type: 'taskList',
        content: [{ type: 'taskItem', attrs: { checked: false }, content: [paragraph()] }],
      },
    ]);
    await expectSame(page);
  });

  test('two texts side by side are read as one, and two marks the editor never sets together are both kept', async ({
    page,
  }) => {
    await open(page);
    await seed(page, THREE);
    await inject(page, [], 0, [
      {
        element: 'paragraph',
        children: [
          { text: [{ insert: 'a' }] },
          { text: [{ insert: 'b' }, { insert: 'c', attributes: { code: {}, bold: {} } }] },
          { text: [{ insert: 'd', attributes: { 'bold--abcdefgh': {} } }] },
        ],
      },
    ]);
    // Only who wrote the second text joins it to the first; here nobody does.
    const after = (await shape(page, 'a')) as { children: Shape[] };
    expect(after.children[0]).toEqual(
      element('paragraph', {}, [
        runs(run('a')),
        runs(run('b'), run('c', { code: {}, bold: {} })),
        runs(run('d', { 'bold--abcdefgh': {} })),
      ]),
    );
    // A member named with a hash is the mark before the hash.
    expect((await stored(page, 'a'))?.content?.[0]).toEqual(
      paragraph(
        text('ab'),
        text('c', { type: 'bold' }, { type: 'code' }),
        text('d', { type: 'bold' }),
      ),
    );
    await expectSame(page);
  });

  test('a node at the root that the document may not hold is wrapped, not deleted, and written back on the next edit', async ({
    page,
  }) => {
    await open(page);
    await seed(page, THREE);
    const wrote = await page.evaluate((userId) => {
      const { spike } = window;
      spike.clearLogs();
      spike.inject([], 0, [{ element: 'mention', attrs: { userId } }]);
      return { a: spike.writes('a'), b: spike.writes('b') };
    }, ADA);
    expect(wrote).toEqual({ a: 0, b: 0 });
    const mention = { type: 'mention', attrs: { userId: ADA } };
    // Shown inside a paragraph the shared content does not have.
    expect(((await shape(page, 'a')) as { children: Shape[] }).children[0]).toEqual(
      element('mention', { userId: ADA }),
    );
    expect((await stored(page, 'a'))?.content?.[0]).toEqual(paragraph(mention));
    await expectSame(page);
    // Whoever edits next writes the paragraph.
    await page.evaluate(() => {
      const { spike } = window;
      spike.a.editor.commands.insertContentAt(spike.range('a', 'paragraph', 1).from + 1, 'E');
    });
    expect(((await shape(page, 'a')) as { children: Shape[] }).children.slice(0, 2)).toEqual([
      element('paragraph', {}, [element('mention', { userId: ADA })]),
      element('paragraph', {}, [runs(run('Efirst'))]),
    ]);
    await expectSame(page);
  });

  test('a text at the root makes every binding throw: the server must not let one through', async ({
    page,
  }) => {
    await open(page);
    await seed(page, THREE);
    const thrown = await page.evaluate(() => {
      window.spike.inject([], 0, [{ text: [{ insert: 'loose' }] }]);
      return window.spike.thrown;
    });
    expect(thrown).toEqual(['a: Method unimplemented', 'b: Method unimplemented']);
    // The shared content has the text; the editors still show what they showed.
    for (const peer of ['a', 'b'] as const) {
      expect(((await shape(page, peer)) as { children: Shape[] }).children[0]).toEqual(
        runs(run('loose')),
      );
      expect(await stored(page, peer)).toEqual(THREE);
    }
  });

  test('a root emptied from elsewhere shows as the empty paragraph again', async ({ page }) => {
    await open(page);
    await seed(page, THREE);
    await page.evaluate(() => {
      window.spike.injectDelete([], 0, 3);
    });
    expect(await shape(page, 'a')).toEqual(fragment());
    expect(await stored(page, 'a')).toBeNull();
    await expectSame(page);
  });

  test('with the content check on, the editors keep it, say so, and stop editing', async ({
    page,
  }) => {
    await open(page, '?check=1');
    await seed(page, THREE);
    const seen = await page.evaluate(() => {
      const { spike } = window;
      spike.clearLogs();
      spike.inject([], 1, [{ element: 'bulletList' }]);
      const editable = spike.a.editor.view.editable;
      spike.a.editor.commands.insertContentAt(1, 'ZZ');
      return {
        editable,
        errors: spike.contentErrors,
        wrote: { a: spike.writes('a'), b: spike.writes('b') },
      };
    });
    expect(((await shape(page, 'a')) as { children: Shape[] }).children).toEqual([
      THREE_SHAPE[0],
      element('bulletList'),
      ...THREE_SHAPE.slice(1),
    ]);
    expect(seen.errors).toEqual([
      'a: Invalid content for node bulletList: <>',
      'b: Invalid content for node bulletList: <>',
    ]);
    expect(seen.editable).toBe(false);
    // Nothing was deleted, and what was typed after that went nowhere.
    expect(seen.wrote).toEqual({ a: 0, b: 0 });
    expect(await stored(page, 'a')).toEqual(THREE);
    await expectSame(page);
  });
});

test('two edits that are each valid merge into a list without entries, which each editor deletes on its own', async ({
  page,
}) => {
  await open(page);
  await seed(page, THREE);
  const apart = await page.evaluate(() => {
    const { spike } = window;
    spike.disconnect();
    spike.a.editor.commands.deleteRange(spike.range('a', 'listItem', 0));
    spike.b.editor.commands.deleteRange(spike.range('b', 'listItem', 1));
    return { a: spike.stored('a'), b: spike.stored('b') };
  });
  const listOf = (entry: string): DocNode =>
    doc(
      paragraph(text('first')),
      { type: 'bulletList', content: [item(paragraph(text(entry)))] },
      paragraph(text('last')),
    );
  expect(apart).toEqual({ a: listOf('two'), b: listOf('one') });

  const together = await page.evaluate(() => {
    const { spike } = window;
    spike.clearLogs();
    spike.connect();
    return {
      wrote: { a: spike.writes('a'), b: spike.writes('b') },
      changes: { a: spike.changes('a'), b: spike.changes('b') },
    };
  });
  // Each was handed the other's deletion before the other's answer to its own,
  // and each answered by deleting the list.
  expect(together.wrote).toEqual({ a: 1, b: 1 });
  const outcome = doc(paragraph(text('first')), paragraph(text('last')));
  expect(await stored(page, 'a')).toEqual(outcome);
  expect(await shape(page, 'a')).toEqual(fragment(THREE_SHAPE[0], THREE_SHAPE[2]));
  await expectSame(page);
  // The list went in the one step that brought the other side's deletion.
  const listGone = {
    remote: true,
    steps: [{ kind: 'replace', from: 7, to: 16, inserted: 0 }],
    before: 22,
    after: 13,
  };
  expect(together.changes).toEqual({ a: [listGone], b: [listGone] });
});

test.describe('undoing', () => {
  const firstLine = async (page: Page): Promise<string | undefined> =>
    (await stored(page, 'a'))?.content?.[0]?.content?.[0]?.text;
  const history = (page: Page) => page.evaluate(() => window.spike.history('a'));

  /** A writes at the start of the first paragraph, B writes at the start of a block. */
  const writeBoth = (page: Page, blockOfB: 'first' | 'last'): Promise<void> =>
    page.evaluate((where) => {
      const { spike } = window;
      spike.a.editor.commands.insertContentAt(1, 'AAA ');
      const at = where === 'first' ? 1 : spike.range('b', 'paragraph', 3).from + 1;
      spike.b.editor.commands.insertContentAt(at, 'BBB ');
    }, blockOfB);
  const undo = (page: Page): Promise<boolean> =>
    page.evaluate(() => window.spike.a.editor.commands.undo());

  test("with the collaboration history alone, takes back one's own change and leaves the other's", async ({
    page,
  }) => {
    const said = await open(page);
    await seed(page, THREE);
    await writeBoth(page, 'first');
    expect(await history(page)).toEqual({ own: 0, shared: { undo: 1, redo: 0 } });
    expect(await undo(page)).toBe(true);
    expect(await firstLine(page)).toBe('BBB first');
    expect(await history(page)).toEqual({ own: 0, shared: { undo: 0, redo: 1 } });
    expect(await undo(page)).toBe(false);
    await expectSame(page);
    expect(await page.evaluate(() => window.spike.a.editor.commands.redo())).toBe(true);
    expect(await firstLine(page)).toBe('BBB AAA first');
    expect(said).toEqual([]);
  });

  test("with the editor's own history beside it, warns, and the undo command goes to the editor's own", async ({
    page,
  }) => {
    const said = await open(page, '?undo=both');
    await seed(page, THREE);
    await expect
      .poll(() => said)
      .toEqual(
        Array.from(
          { length: 2 },
          () =>
            '[tiptap warn]: "@tiptap/extension-collaboration" comes with its own history support and is not compatible with "@tiptap/extension-undo-redo".',
        ),
      );
    // The other's change is in another block: the editor's own history takes
    // the change back, and the collaboration's is not asked (nothing to redo
    // there; it saw the taking back as one more change to undo).
    await writeBoth(page, 'last');
    expect(await history(page)).toEqual({ own: 1, shared: { undo: 1, redo: 0 } });
    expect(await undo(page)).toBe(true);
    expect(await firstLine(page)).toBe('first');
    const after = await history(page);
    expect(after.own).toBe(0);
    expect(after.shared.redo).toBe(0);
    await expectSame(page);
  });

  test("with the editor's own history beside it, an undo after the other's change to the same block says it did and does nothing", async ({
    page,
  }) => {
    await open(page, '?undo=both');
    await seed(page, THREE);
    await writeBoth(page, 'first');
    expect(await undo(page)).toBe(true);
    expect(await firstLine(page)).toBe('BBB AAA first');
    expect(await undo(page)).toBe(false);
    await expectSame(page);
  });
});

test("the caret extension needs a provider only for its awareness, and shows the other's caret with its name", async ({
  page,
}) => {
  const said = await open(page);
  await seed(page, THREE);
  // Each side knows of the other before anybody has a caret.
  expect(
    await page.evaluate(() => {
      const { spike } = window;
      const names = (peer: 'a' | 'b'): string[] =>
        spike[peer].editor.storage.collaborationCaret.users
          .map(({ name }) => name as string)
          .sort();
      return { a: names('a'), b: names('b') };
    }),
  ).toEqual({ a: ['Ada', 'Bora'], b: ['Ada', 'Bora'] });
  const caretOf = (peer: 'a' | 'b') =>
    page.getByTestId(`peer-${peer}`).locator('.collaboration-carets__caret');
  await expect(caretOf('a')).toHaveCount(0);
  await expect(caretOf('b')).toHaveCount(0);

  await page.getByTestId('peer-a').locator('.ProseMirror').click();
  await page.keyboard.type('x');
  await expect(caretOf('b')).toHaveCount(1);
  await expect(caretOf('b').locator('.collaboration-carets__label')).toHaveText('Ada');
  // An editor draws no caret for itself.
  await expect(caretOf('a')).toHaveCount(0);

  // What a peer tells the others: who it is, and where its selection is as
  // positions relative to the shared content.
  const told = await page.evaluate(() => {
    const { spike } = window;
    return spike.b.awareness.getStates().get(spike.a.doc.clientID);
  });
  expect(told?.user).toEqual({ name: 'Ada', color: '#b4532a' });
  expect(Object.keys(told ?? {}).sort()).toEqual(['cursor', 'user']);
  expect(Object.keys((told?.cursor ?? {}) as object).sort()).toEqual(['anchor', 'head']);

  // The other way round, once the other editor is the one written in.
  await page.getByTestId('peer-b').locator('.ProseMirror').click();
  await expect(caretOf('a')).toHaveCount(1);
  await expect(caretOf('a').locator('.collaboration-carets__label')).toHaveText('Bora');
  expect(said).toEqual([]);
});

test('the nodes of our own and a diagram go from one editor to the other and back', async ({
  page,
}) => {
  const said = await open(page);
  await seed(page, doc(paragraph(text('start'))));
  await page.evaluate((userId) => {
    const { spike } = window;
    const end = spike.a.editor.state.doc.content.size;
    spike.a.editor.commands.insertContentAt(end, [
      {
        type: 'paragraph',
        content: [
          { type: 'mention', attrs: { userId } },
          { type: 'text', text: ' and ' },
          { type: 'inlineMath', attrs: { latex: 'x^2' } },
        ],
      },
      { type: 'image', attrs: { fileId: 'file-a.png' } },
      { type: 'blockMath', attrs: { latex: 'E=mc^2' } },
      {
        type: 'codeBlock',
        attrs: { language: 'mermaid' },
        content: [{ type: 'text', text: 'graph TD\n  A-->B' }],
      },
    ]);
  }, ADA);
  const written = doc(
    paragraph(text('start')),
    paragraph({ type: 'mention', attrs: { userId: ADA } }, text(' and '), {
      type: 'inlineMath',
      attrs: { latex: 'x^2' },
    }),
    { type: 'image', attrs: { fileId: 'file-a.png' } },
    { type: 'blockMath', attrs: { latex: 'E=mc^2' } },
    { type: 'codeBlock', attrs: { language: 'mermaid' }, content: [text('graph TD\n  A-->B')] },
  );
  expect(await stored(page, 'b')).toEqual(written);
  await expectSame(page);

  // The other editor draws them as its own.
  const b = page.getByTestId('peer-b');
  await expect(b.locator('span[data-mention]')).toHaveAttribute('data-user-id', ADA);
  await expect(b.locator('span.doc-math[data-type="inline-math"]')).toHaveAttribute(
    'data-latex',
    'x^2',
  );
  await expect(b.locator('div.doc-math[data-type="block-math"]')).toHaveAttribute(
    'data-latex',
    'E=mc^2',
  );
  await expect(b.locator('img[data-file-id="file-a.png"]')).toHaveCount(1);
  // The diagram library arrives on its own, which takes a while on a busy machine.
  await expect(b.locator('.doc-diagram[data-diagram] svg')).toHaveCount(1, { timeout: 60_000 });

  // And a change of an attribute alone comes back.
  await page.evaluate(() => {
    const { spike } = window;
    const { view } = spike.b.editor;
    view.dispatch(
      view.state.tr
        .setNodeMarkup(spike.range('b', 'blockMath', 0).from, undefined, { latex: 'a^2+b^2' })
        .setNodeMarkup(spike.range('b', 'image', 0).from, undefined, {
          fileId: 'file-a.png',
          alt: 'a picture',
        }),
    );
  });
  const changed = (await stored(page, 'a'))?.content;
  expect(changed?.[2]).toEqual({
    type: 'image',
    attrs: { fileId: 'file-a.png', alt: 'a picture' },
  });
  expect(changed?.[3]).toEqual({ type: 'blockMath', attrs: { latex: 'a^2+b^2' } });
  await expect(
    page.getByTestId('peer-a').locator('div.doc-math[data-type="block-math"]'),
  ).toHaveAttribute('data-latex', 'a^2+b^2');
  await expectSame(page);
  expect(said).toEqual([]);
});
