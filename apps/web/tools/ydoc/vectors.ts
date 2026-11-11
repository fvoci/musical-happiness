/**
 * Writes `contracts/ydoc-vectors.json`: bodies as the Y documents the
 * editor's binding makes of them, for the server's conversion
 * (`crates/collab`) to be held against.
 *
 *   node tools/ydoc/vectors.ts           writes the file
 *   node tools/ydoc/vectors.ts --check   fails when the file is not what would be written
 *
 * The server keeps a body as JSON and, while people edit it together, as a Y
 * document; it converts between the two without the editor's library. What
 * that library does is written down here by running it:
 *
 * - `documents`: every body the shared vectors (`contracts/body-vectors.json`)
 *   say is accepted, as the update of the Y document made from it, and the
 *   JSON an editor reads from that document in the form that is stored.
 * - `readings`: shared content no editor writes but every editor keeps, and
 *   the JSON read from it (cases.ts).
 * - `repairs`: shared content that does not fit the schema, and the JSON of
 *   what is left once an editor has deleted what fails.
 * - `schema`: the marks in the schema's order and the attributes each node
 *   and mark has, with the value an editor gives one that is left out.
 *
 * A document is made by a client whose identifier is fixed, so that the
 * file is the same whenever it is written.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { Node, type Schema } from '@tiptap/pm/model';
import { prosemirrorToYXmlFragment, yXmlFragmentToProseMirrorRootNode } from '@tiptap/y-tiptap';
import * as Y from 'yjs';

import {
  BODY_VERSION,
  isEmptyDoc,
  toStoredDoc,
  validateDoc,
  type DocNode,
} from '../../src/lib/editor/schema.ts';

import { READINGS, REPAIRS, type RawCase } from './cases.ts';
import { rawType, type RawNode } from './shape.ts';

/** The shared type an editor is bound to: the collaboration extension's own name for it. */
export const FIELD = 'default';

const CONTRACTS = path.resolve(import.meta.dirname, '../../../../contracts');
export const BODY_VECTORS_FILE = path.join(CONTRACTS, 'body-vectors.json');
export const YDOC_VECTORS_FILE = path.join(CONTRACTS, 'ydoc-vectors.json');

const DESCRIPTION =
  'Bodies as Y documents, written by apps/web/tools/ydoc/vectors.ts from the ' +
  "editor's own binding (@tiptap/y-tiptap); the server's conversion (crates/collab) and the web " +
  "app's tests read it. Do not edit by hand. field is the name of the XML fragment at the root of " +
  'the Y document. An update is a whole Y document as a version 1 update in base64, and json is ' +
  'what an editor reads from it in the form that is stored (null for no body). documents: every ' +
  'accepted case of body-vectors.json with a body (not those with a build), by its name; json is ' +
  'that body as it comes back, which differs where the editor fills in an attribute (the start ' +
  'of an ordered list) or orders marks. readings: shared content no editor writes and every ' +
  'editor keeps, and how it is read. repairs: shared content that does not fit the schema; json ' +
  'is what is left when an editor has deleted what fails, from the bottom up. schema: the marks ' +
  'in the order the editor stores them, and the attributes of each mark and node with the value ' +
  'given to one that is left out (null: none).';

/**
 * The identifiers of the clients that write the documents. Whoever reads, or
 * holds what others wrote, is another: a document that is handed its own
 * identifier's writing takes a new one at random.
 */
const WRITER = 1;
const HOLDER = 8;
const READER = 9;

export interface YdocVector {
  name: string;
  update: string;
  json: DocNode | null;
}

export interface YdocVectors {
  description: string;
  bodyVersion: number;
  field: string;
  schema: {
    marks: { name: string; attrs: Record<string, unknown> }[];
    nodes: Record<string, Record<string, unknown>>;
  };
  documents: YdocVector[];
  readings: YdocVector[];
  repairs: YdocVector[];
}

interface BodyCase {
  name: string;
  body?: unknown;
  expect: string;
}

const base64 = (bytes: Uint8Array): string => Buffer.from(bytes).toString('base64');

/** The bytes of an update written in base64. */
export const fromBase64 = (text: string): Uint8Array => new Uint8Array(Buffer.from(text, 'base64'));

function newDoc(client: number): Y.Doc {
  const doc = new Y.Doc();
  doc.clientID = client;
  return doc;
}

/**
 * What an editor reads from a Y document, in the form that is stored. The
 * document is the reader's own copy, and reading changes it: what does not
 * fit the schema is deleted from it, as the binding of every editor does.
 */
export function readStored(doc: Y.Doc, schema: Schema): DocNode | null {
  const root = yXmlFragmentToProseMirrorRootNode(doc.getXmlFragment(FIELD), schema);
  if (root.childCount === 0) return null;
  const stored = toStoredDoc(root.toJSON() as DocNode);
  return isEmptyDoc(stored) ? null : stored;
}

/** A reader's copy of the Y document an update holds. */
export function readerOf(update: Uint8Array): Y.Doc {
  const doc = newDoc(READER);
  Y.applyUpdate(doc, update);
  return doc;
}

const sameBytes = (one: Uint8Array, other: Uint8Array): boolean =>
  one.length === other.length && one.every((byte, index) => byte === other[index]);

/** The vector of a Y document: its update and what an editor makes of it. */
function vectorOf(name: string, written: Y.Doc, schema: Schema, deletes: boolean): YdocVector {
  const update = Y.encodeStateAsUpdate(written);
  const reader = readerOf(update);
  const json = readStored(reader, schema);
  const after = Y.encodeStateAsUpdate(reader);
  if (sameBytes(update, after) === deletes) {
    throw new Error(
      `${name}: the editor ${deletes ? 'deleted nothing' : 'deleted something'} when it read`,
    );
  }
  const again = readStored(reader, schema);
  if (
    JSON.stringify(again) !== JSON.stringify(json) ||
    !sameBytes(after, Y.encodeStateAsUpdate(reader))
  )
    throw new Error(`${name}: reading a second time changed what was read`);
  const problems = json === null ? [] : validateDoc(json);
  if (problems.length > 0)
    throw new Error(`${name}: what the editor reads is not a body: ${problems.join('; ')}`);
  return { name, update: base64(update), json };
}

function rawDoc(root: RawNode[], client = WRITER): Y.Doc {
  const doc = newDoc(client);
  doc.getXmlFragment(FIELD).insert(0, root.map(rawType));
  return doc;
}

/** A copy of a Y document for another client to edit. */
function fork(base: Y.Doc, client: number): Y.Doc {
  const doc = newDoc(client);
  Y.applyUpdate(doc, Y.encodeStateAsUpdate(base));
  return doc;
}

/** The base with the edits of two clients, each made without the other's. */
function merged(
  base: Y.Doc,
  first: (root: Y.XmlFragment) => void,
  second: (root: Y.XmlFragment) => void,
  schema: Schema,
  name: string,
): Y.Doc {
  const result = fork(base, HOLDER);
  [first, second].forEach((edit, index) => {
    const side = fork(base, WRITER + 1 + index);
    edit(side.getXmlFragment(FIELD));
    // Each edit alone leaves a document every editor keeps as it is.
    const alone = Y.encodeStateAsUpdate(side);
    const reader = readerOf(alone);
    readStored(reader, schema);
    if (!sameBytes(alone, Y.encodeStateAsUpdate(reader)))
      throw new Error(`${name}: one of the two edits is not valid alone`);
    Y.applyUpdate(result, alone);
  });
  return result;
}

const childOf = (parent: Y.XmlFragment, index: number): Y.XmlFragment => {
  const child = parent.get(index);
  if (!(child instanceof Y.XmlFragment)) throw new Error(`no element at ${index}`);
  return child;
};

/** Shared content that two valid edits leave when they are merged. */
function mergedRepairs(schema: Schema): YdocVector[] {
  const text = (insert: string): RawNode => ({ text: [{ insert }] });
  const paragraph = (insert: string): RawNode => ({
    element: 'paragraph',
    children: [text(insert)],
  });
  const entry = (...children: RawNode[]): RawNode => ({ element: 'listItem', children });
  const made: [string, Y.Doc][] = [];

  const emptied = 'two people each delete one of the two entries of a list';
  made.push([
    emptied,
    merged(
      rawDoc([
        paragraph('a'),
        { element: 'bulletList', children: [entry(paragraph('one')), entry(paragraph('two'))] },
      ]),
      (root) => {
        childOf(root, 1).delete(0, 1);
      },
      (root) => {
        childOf(root, 1).delete(1, 1);
      },
      schema,
      emptied,
    ),
  ]);

  const headless =
    'one deletes the first paragraph of an entry, the other puts a quote in place of the second';
  made.push([
    headless,
    merged(
      rawDoc([
        {
          element: 'bulletList',
          children: [entry(paragraph('one'), paragraph('more')), entry(paragraph('two'))],
        },
      ]),
      (root) => {
        childOf(childOf(root, 0), 0).delete(0, 1);
      },
      (root) => {
        const first = childOf(childOf(root, 0), 0);
        first.delete(1, 1);
        first.insert(1, [rawType({ element: 'blockquote', children: [paragraph('q')] })]);
      },
      schema,
      headless,
    ),
  ]);

  return made.map(([name, doc]) => vectorOf(name, doc, schema, true));
}

/** The attributes a kind of node or mark has, with the value given to one left out. */
function attributesOf(
  attrs: Record<string, { default?: unknown }> | undefined,
): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(attrs ?? {}).map(([name, spec]) => [name, spec.default ?? null]),
  );
}

/** Everything the file holds, from the editor's schema and the accepted bodies. */
export function buildVectors(schema: Schema, bodyCases: BodyCase[]): YdocVectors {
  const documents = bodyCases
    .filter((one) => one.expect === 'accept' && one.body !== undefined)
    .map((one) => {
      const doc = newDoc(WRITER);
      prosemirrorToYXmlFragment(Node.fromJSON(schema, one.body), doc.getXmlFragment(FIELD));
      // That an editor keeps every one of them as it is, is checked as it is read.
      return vectorOf(one.name, doc, schema, false);
    });
  const raw = (cases: RawCase[], deletes: boolean): YdocVector[] =>
    cases.map((one) => vectorOf(one.name, rawDoc(one.root), schema, deletes));
  return {
    description: DESCRIPTION,
    bodyVersion: BODY_VERSION,
    field: FIELD,
    schema: {
      marks: Object.entries(schema.marks).map(([name, type]) => ({
        name,
        attrs: attributesOf(type.spec.attrs),
      })),
      nodes: Object.fromEntries(
        Object.entries(schema.nodes)
          .map(([name, type]) => [name, attributesOf(type.spec.attrs)] as const)
          .sort(([one], [other]) => (one < other ? -1 : 1)),
      ),
    },
    documents,
    readings: raw(READINGS, false),
    repairs: [...mergedRepairs(schema), ...raw(REPAIRS, true)],
  };
}

/** The accepted and refused bodies both sides are held to. */
export function readBodyCases(): BodyCase[] {
  return (JSON.parse(readFileSync(BODY_VECTORS_FILE, 'utf8')) as { cases: BodyCase[] }).cases;
}

/** The text of the file for what it holds. */
export const serialize = (vectors: YdocVectors): string => `${JSON.stringify(vectors, null, 2)}\n`;

/**
 * The schema of the product's editor. Its extensions reach the app's texts
 * through an alias only the build knows, so they are loaded as the build
 * loads them.
 */
async function productSchema(): Promise<Schema> {
  const { createServer } = await import('vite');
  const root = path.resolve(import.meta.dirname, '../..');
  const server = await createServer({
    root,
    configFile: path.join(root, 'vite.config.ts'),
    server: { middlewareMode: true, hmr: false, ws: false },
    appType: 'custom',
    logLevel: 'error',
  });
  try {
    const editor = (await server.ssrLoadModule('/src/lib/editor/extensions.ts')) as {
      bodyExtensions: () => unknown[];
    };
    const core = (await server.ssrLoadModule('@tiptap/core')) as {
      getSchema: (extensions: unknown[]) => Schema;
    };
    return core.getSchema(editor.bodyExtensions());
  } finally {
    await server.close();
  }
}

if (import.meta.main) {
  const written = serialize(buildVectors(await productSchema(), readBodyCases()));
  if (process.argv.includes('--check')) {
    let committed = '';
    try {
      committed = readFileSync(YDOC_VECTORS_FILE, 'utf8');
    } catch {
      // No file is a stale file.
    }
    if (committed !== written) {
      console.error('contracts/ydoc-vectors.json is not what tools/ydoc/vectors.ts writes');
      process.exit(1);
    }
    console.log('contracts/ydoc-vectors.json: up to date');
  } else {
    writeFileSync(YDOC_VECTORS_FILE, written);
    console.log(`wrote ${path.relative(process.cwd(), YDOC_VECTORS_FILE)}`);
  }
}
