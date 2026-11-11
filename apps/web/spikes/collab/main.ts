/**
 * Two editors of the product's body, each bound to a Y document of its own,
 * and a relay in the page that carries each document's updates to the other.
 * There is no server and no socket: what is tried out here is the shape of
 * the shared document and what the editor does with changes that arrive.
 *
 *   ?undo=both   keep the editor's own history beside the collaboration's
 *   ?check=1     make the editors with their content check on
 */
import { Editor, Extension } from '@tiptap/core';
import { Collaboration, isChangeOrigin } from '@tiptap/extension-collaboration';
import { CollaborationCaret } from '@tiptap/extension-collaboration-caret';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import { prosemirrorJSONToYDoc } from '@tiptap/y-tiptap';
import { applyAwarenessUpdate, Awareness, encodeAwarenessUpdate } from 'y-protocols/awareness';
import * as Y from 'yjs';

import './spike.css';
import '../../src/lib/editor/editor.css';

import { bodyExtensions } from '$lib/editor/extensions.ts';
import { setFileUrlResolver } from '$lib/editor/files.ts';
import { isEmptyDoc, toStoredDoc, type DocNode } from '$lib/editor/schema.ts';

import { rawType, shapeOf, typeAt, type RawElement } from '../../tools/ydoc/shape.ts';
import { fakeFileUrl } from '../editor/fixtures.ts';

import type { ChangeRecord, MarkerState, Peer, PeerName, SpikeApi } from './api.ts';

/** The shared type the collaboration extension binds to when it is told nothing else. */
const FIELD = 'default';

const query = new URLSearchParams(location.search);
const keepOwnHistory = query.get('undo') === 'both';
const contentCheck = query.get('check') === '1';

setFileUrlResolver(fakeFileUrl);

/** What marks an update as one the relay brought, so that it is not sent back. */
const RELAY = Symbol('relay');

interface MarkerPluginState {
  position: number | null;
  deleted: boolean;
  set: DecorationSet;
}

const markerKey = new PluginKey<MarkerPluginState>('spikeMarker');

/**
 * A place kept the two ways the product keeps one: as a position carried
 * through each transaction's mapping, and as a decoration the editor maps.
 */
const Marker = Extension.create({
  name: 'spikeMarker',
  addProseMirrorPlugins() {
    return [
      new Plugin<MarkerPluginState>({
        key: markerKey,
        state: {
          init: () => ({ position: null, deleted: false, set: DecorationSet.empty }),
          apply(transaction, state) {
            const placed = transaction.getMeta(markerKey) as number | undefined;
            if (placed !== undefined) {
              const widget = Decoration.widget(
                placed,
                (view) => {
                  const element = view.dom.ownerDocument.createElement('span');
                  element.dataset.marker = '';
                  return element;
                },
                { key: 'spike-marker' },
              );
              return {
                position: placed,
                deleted: false,
                set: DecorationSet.create(transaction.doc, [widget]),
              };
            }
            if (state.position === null) return state;
            const mapped = transaction.mapping.mapResult(state.position);
            return {
              position: mapped.pos,
              deleted: state.deleted || mapped.deleted,
              set: state.set.map(transaction.mapping, transaction.doc),
            };
          },
        },
        props: { decorations: (state) => markerKey.getState(state)?.set },
      }),
    ];
  },
});

const changes: Record<PeerName, ChangeRecord[]> = { a: [], b: [] };
const contentErrors: string[] = [];

function makePeer(name: PeerName, user: { name: string; color: string }): Peer {
  const host = document.getElementById(`peer-${name}`);
  if (!host) throw new Error(`index.html has no #peer-${name} element`);
  const doc = new Y.Doc();
  const awareness = new Awareness(doc);
  const editor = new Editor({
    element: host,
    injectCSS: false,
    enableContentCheck: contentCheck,
    extensions: [
      ...bodyExtensions().filter((extension) => keepOwnHistory || extension.name !== 'undoRedo'),
      Collaboration.configure({ document: doc, field: FIELD }),
      // All the caret extension is given of a provider is its awareness.
      CollaborationCaret.configure({ provider: { awareness }, user }),
      Marker,
    ],
    editorProps: { attributes: { class: 'doc-body', 'aria-label': `peer ${name}` } },
    onContentError: ({ error }) => {
      contentErrors.push(`${name}: ${error.message}`);
    },
    onTransaction: ({ transaction }) => {
      if (!transaction.docChanged) return;
      changes[name].push({
        remote: isChangeOrigin(transaction),
        steps: transaction.steps.map((step) => {
          const json = step.toJSON() as {
            stepType: string;
            from?: number;
            to?: number;
            slice?: { content?: unknown[] };
          };
          const map = step.getMap();
          let inserted = 0;
          map.forEach((_oldStart, _oldEnd, newStart, newEnd) => {
            inserted += newEnd - newStart;
          });
          return { kind: json.stepType, from: json.from ?? null, to: json.to ?? null, inserted };
        }),
        before: transaction.before.content.size,
        after: transaction.doc.content.size,
      });
    },
  });
  return { editor, doc, awareness };
}

const a = makePeer('a', { name: 'Ada', color: '#b4532a' });
const b = makePeer('b', { name: 'Bora', color: '#2a6fb4' });
const peers: Record<PeerName, Peer> = { a, b };

let connected = true;

function relay(from: Peer, to: Peer): void {
  from.doc.on('update', (update: Uint8Array, origin: unknown) => {
    if (origin === RELAY || !connected) return;
    Y.applyUpdate(to.doc, update, RELAY);
  });
  from.awareness.on(
    'update',
    (
      { added, updated, removed }: { added: number[]; updated: number[]; removed: number[] },
      origin: unknown,
    ) => {
      if (origin === RELAY || !connected) return;
      const update = encodeAwarenessUpdate(from.awareness, [...added, ...updated, ...removed]);
      applyAwarenessUpdate(to.awareness, update, RELAY);
    },
  );
}
relay(a, b);
relay(b, a);

/** Gives `to` what `from` has and it has not. */
function bringUpToDate(from: Y.Doc, to: Y.Doc): void {
  Y.applyUpdate(to, Y.encodeStateAsUpdate(from, Y.encodeStateVector(to)), RELAY);
}

/** A document no editor is bound to, brought up to date with A, changed, and handed to both. */
function fromOutside(change: (root: Y.XmlFragment) => void): void {
  const outside = new Y.Doc();
  bringUpToDate(a.doc, outside);
  const before = Y.encodeStateVector(outside);
  change(outside.getXmlFragment(FIELD));
  const update = Y.encodeStateAsUpdate(outside, before);
  Y.applyUpdate(a.doc, update, RELAY);
  Y.applyUpdate(b.doc, update, RELAY);
}

const api: SpikeApi = {
  a,
  b,
  field: FIELD,
  contentCheck,
  contentErrors,
  seed(doc: DocNode): void {
    const made = prosemirrorJSONToYDoc(a.editor.schema, doc, FIELD);
    const update = Y.encodeStateAsUpdate(made);
    Y.applyUpdate(a.doc, update, RELAY);
    Y.applyUpdate(b.doc, update, RELAY);
  },
  stored(peer: PeerName): DocNode | null {
    const stored = toStoredDoc(peers[peer].editor.getJSON() as DocNode);
    return isEmptyDoc(stored) ? null : stored;
  },
  json: (peer: PeerName): DocNode => peers[peer].editor.getJSON() as DocNode,
  roots: (peer: PeerName) =>
    Array.from(peers[peer].doc.share.entries(), ([name, type]) => ({
      name,
      kind: type.constructor.name,
    })),
  shape: (peer: PeerName) => shapeOf(peers[peer].doc.getXmlFragment(FIELD)),
  disconnect(): void {
    connected = false;
  },
  connect(): void {
    connected = true;
    bringUpToDate(a.doc, b.doc);
    bringUpToDate(b.doc, a.doc);
  },
  range(peer: PeerName, type: string, index: number): { from: number; to: number } {
    let seen = 0;
    let found: { from: number; to: number } | undefined;
    peers[peer].editor.state.doc.descendants((node, position) => {
      if (found) return false;
      if (node.type.name === type) {
        if (seen === index) found = { from: position, to: position + node.nodeSize };
        seen += 1;
      }
      return true;
    });
    if (!found) throw new Error(`peer ${peer} has no ${type} number ${index}`);
    return found;
  },
  changes: (peer: PeerName) => changes[peer],
  clearChanges(): void {
    changes.a = [];
    changes.b = [];
  },
  placeMarker(peer: PeerName, position: number): void {
    const { view } = peers[peer].editor;
    view.dispatch(view.state.tr.setMeta(markerKey, position));
  },
  marker(peer: PeerName): MarkerState {
    const state = markerKey.getState(peers[peer].editor.state);
    const decoration = state?.set.find()[0]?.from ?? null;
    return { position: state?.position ?? null, deleted: state?.deleted ?? false, decoration };
  },
  inject(path: number[], index: number, nodes): void {
    fromOutside((root) => {
      const parent = typeAt(root, path);
      if (parent instanceof Y.XmlText) throw new Error('a text takes no nodes');
      parent.insert(index, nodes.map(rawType));
    });
  },
  injectAttribute(path: number[], name: string, value): void {
    fromOutside((root) => {
      const element = typeAt(root, path);
      if (!(element instanceof Y.XmlElement)) throw new Error('only an element has attributes');
      (element as unknown as RawElement).setAttribute(name, value);
    });
  },
  injectDelete(path: number[], index: number, length: number): void {
    fromOutside((root) => {
      const parent = typeAt(root, path);
      parent.delete(index, length);
    });
  },
  markOrder: () => Object.keys(a.editor.schema.marks),
};

window.spike = api;
document.body.dataset.ready = '';
