/**
 * What one letter another client typed costs the editor that receives it, on a body
 * at the limit of nodes. The editor is bound to a shared document with what
 * `BodyEditor` binds it with: the collaboration extension on the body's field,
 * `MovesKept`, and the body's extensions without history. A second document, the
 * server's, writes a letter at the end, which reaches the editor's document as an
 * update. Opening the editor is not measured; each letter is, from the update to the
 * end of the transaction it makes in the editor, and to the frame that draws it.
 *
 * Left out of what a card's body has: the others' carets (`caretsOf`, whose plugin
 * draws its decorations again at every change), `readsFirst` (the page read before
 * each update the room applies), the upload and slash-menu extensions, `setMayWrite`,
 * the listener that sees the content fit again, and `BodyEditor`'s own
 * `onTransaction`. The editor has no focus, the letter lands at the end, out of
 * sight, and the update comes in with an origin of the page's, not through the
 * room's provider.
 *
 *   ?shape=paragraphs   24,999 paragraphs of a text each (the default)
 *   ?shape=marks        paragraphs of 200 letters, every other with four marks
 *   ?check=1            the editor checks the content of every transaction from the
 *                       shared document, as `BodyEditor` makes it (`enableContentCheck`)
 */
import { Editor, getSchema } from '@tiptap/core';
import { Collaboration } from '@tiptap/extension-collaboration';
import { prosemirrorJSONToYDoc } from '@tiptap/y-tiptap';
import * as Y from 'yjs';

import '../../src/lib/editor/editor.css';

import { BODY_FIELD } from '$lib/editor/body-editor.ts';
import { bodyExtensions } from '$lib/editor/extensions.ts';
import { MovesKept } from '$lib/editor/moves-kept.ts';
import { MAX_BODY_BYTES, MAX_NODES, toStoredDoc, type DocNode } from '$lib/editor/schema.ts';

import type { Measured, Opened, RemoteSpike, Runs } from './api.ts';

const query = new URLSearchParams(location.search);
const check = query.get('check') === '1';
const shape = query.get('shape') ?? 'paragraphs';

/** What marks an update as the server's: an origin that is not the binding's, as the provider's is not. */
const SERVER = Symbol('server');

const text = (value: string, marks?: DocNode['marks']): DocNode =>
  marks ? { type: 'text', text: value, marks } : { type: 'text', text: value };
const paragraph = (...content: DocNode[]): DocNode => ({ type: 'paragraph', content });

/** The bytes of a node as stored JSON. */
const sizeOf = (node: DocNode): number => new TextEncoder().encode(JSON.stringify(node)).length;

/** The body of the shape, as many nodes as a body may have, within its bytes. */
function body(): DocNode {
  const nodes = MAX_NODES - 1;
  if (shape === 'paragraphs') {
    return {
      type: 'doc',
      content: Array.from({ length: Math.floor(nodes / 2) }, () =>
        paragraph(text('a paragraph of the document')),
      ),
    };
  }
  if (shape !== 'marks') throw new Error(`no shape is named ${shape}`);
  // The code mark excludes the others in the editor's schema; the four that go together.
  const marked = text('a', [
    { type: 'bold' },
    { type: 'italic' },
    { type: 'strike' },
    { type: 'link', attrs: { href: '/' } },
  ]);
  const letters = Array.from({ length: 200 }, (_, at) => (at % 2 === 0 ? marked : text('a')));
  const one = paragraph(...letters);
  const count = Math.min(
    Math.floor((MAX_BODY_BYTES - 1024) / sizeOf(one)),
    Math.floor(nodes / 201),
  );
  return { type: 'doc', content: Array.from({ length: count }, () => one) };
}

/** The middle, least and most of several times. */
function runsOf(times: number[]): Runs {
  const sorted = [...times].sort((one, other) => one - other);
  return {
    least: sorted[0] ?? Number.NaN,
    middle: sorted[Math.floor(sorted.length / 2)] ?? Number.NaN,
    most: sorted.at(-1) ?? Number.NaN,
  };
}

function timedRuns(runs: number, run: () => unknown): Runs {
  const times: number[] = [];
  for (let at = 0; at < runs; at += 1) {
    const start = performance.now();
    run();
    times.push(performance.now() - start);
  }
  return runsOf(times);
}

/** How long the editor has to draw the body: less than the test waits. */
const OPENING = 240_000;

/** The second frame from now: the one after the browser drew what changed. */
const drawn = (): Promise<void> =>
  new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        resolve();
      });
    });
  });

const stored = body();
const extensions = bodyExtensions({ history: false });
const server = prosemirrorJSONToYDoc(getSchema(extensions), stored, BODY_FIELD);
const shared = new Y.Doc();
Y.applyUpdate(shared, Y.encodeStateAsUpdate(server), SERVER);

const contentErrors: string[] = [];
let transactions = 0;
let updates = 0;

const host = document.getElementById('editor');
if (!host) throw new Error('index.html has no #editor element');
const editor = new Editor({
  element: host,
  injectCSS: false,
  enableContentCheck: check,
  onContentError: ({ error }) => {
    contentErrors.push(error.message);
  },
  extensions: [
    Collaboration.configure({ document: shared, field: BODY_FIELD }),
    MovesKept.configure({}),
    ...extensions,
  ],
  editorProps: { attributes: { class: 'doc-body', 'aria-label': 'body' } },
  onTransaction: () => {
    transactions += 1;
  },
  // `BodyEditor` hands what it stores to `onChange`, which a card's body does not give.
  onUpdate: () => {
    updates += 1;
  },
});

/** The update of a letter the server writes at the end of the last paragraph. */
function letter(): Uint8Array {
  const before = Y.encodeStateVector(server);
  const fragment = server.getXmlFragment(BODY_FIELD);
  const last = fragment.get(fragment.length - 1);
  if (!(last instanceof Y.XmlElement)) throw new Error('the body ends with a paragraph');
  const words = last.get(last.length - 1);
  if (!(words instanceof Y.XmlText)) throw new Error('the last paragraph ends with a text');
  words.insert(words.length, 'x');
  return Y.encodeStateAsUpdate(server, before);
}

async function measure(runs: number, warm: number): Promise<Measured> {
  const applied: number[] = [];
  const shown: number[] = [];
  let counted = { transactions: 0, updates: 0 };
  for (let at = 0; at < warm + runs; at += 1) {
    const update = letter();
    const before = { transactions, updates };
    const start = performance.now();
    Y.applyUpdate(shared, update, SERVER);
    const end = performance.now();
    await drawn();
    const seen = performance.now();
    if (at < warm) continue;
    applied.push(end - start);
    shown.push(seen - start);
    counted = {
      transactions: counted.transactions + transactions - before.transactions,
      updates: counted.updates + updates - before.updates,
    };
  }
  const letters = 'x'.repeat(warm + runs);
  return {
    applied: runsOf(applied),
    drawn: runsOf(shown),
    transactions: counted.transactions / runs,
    updates: counted.updates / runs,
    arrived: editor.state.doc.lastChild?.textContent.endsWith(letters) ?? false,
  };
}

async function open(): Promise<Opened> {
  // The binding draws the shared document into the editor as the view is made.
  const until = performance.now() + OPENING;
  while (editor.state.doc.childCount !== stored.content?.length) {
    if (performance.now() > until) throw new Error(`the editor drew no body in ${OPENING} ms`);
    await drawn();
  }
  await drawn();
  let nodes = 1;
  editor.state.doc.descendants(() => {
    nodes += 1;
  });
  return { nodes, bytes: sizeOf(stored), check };
}

const spike: RemoteSpike = {
  ready: open(),
  measure,
  checked: (runs) =>
    timedRuns(runs, () => {
      editor.state.doc.check();
    }),
  stored: (runs) => timedRuns(runs, () => toStoredDoc(editor.getJSON() as DocNode)),
  contentErrors: () => [...contentErrors],
};
(window as unknown as { spike: RemoteSpike }).spike = spike;
