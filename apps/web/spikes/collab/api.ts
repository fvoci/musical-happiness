/**
 * What the collaboration spike's page offers its browser tests, as
 * `window.spike`: two editors, each on a Y document of its own, a relay
 * between the two documents that can be cut, and a third document nobody
 * edits by hand, through which a test writes shared content no editor would.
 */
import type { Editor } from '@tiptap/core';
import type { Awareness } from 'y-protocols/awareness';
import type * as Y from 'yjs';

import type { DocNode } from '$lib/editor/schema.ts';

import type { AttributeValue, RawNode, Shape } from '../../tools/ydoc/shape.ts';

export type PeerName = 'a' | 'b';

export interface Peer {
  editor: Editor;
  doc: Y.Doc;
  awareness: Awareness;
}

/** One step of a transaction that changed an editor's document. */
export interface StepRecord {
  kind: string;
  from: number | null;
  to: number | null;
  /** The size of what the step puts in. */
  inserted: number;
}

/** A transaction that changed an editor's document. */
export interface ChangeRecord {
  /** Whether it came from the shared document, not from this editor's user. */
  remote: boolean;
  steps: StepRecord[];
  /** The size of the document's content before and after. */
  before: number;
  after: number;
}

/** Where a place that was marked is now, as the editor's own mapping has it. */
export interface MarkerState {
  /** The position, carried through every transaction's mapping. */
  position: number | null;
  /** Whether a mapping said the place itself was deleted. */
  deleted: boolean;
  /** Where the decoration put at the place is; `null` when the mapping dropped it. */
  decoration: number | null;
}

export interface SpikeApi {
  a: Peer;
  b: Peer;
  /** The name of the shared type the editors are bound to. */
  field: string;
  /** Whether the editors were made with the editor's own content check. */
  contentCheck: boolean;
  /** Puts a document made outside the editors, as a server would make it, into both. */
  seed: (doc: DocNode) => void;
  /** What an editor would store now; `null` for an empty body. */
  stored: (peer: PeerName) => DocNode | null;
  /** The editor's document as it is, with nothing left out. */
  json: (peer: PeerName) => DocNode;
  /** The names and kinds of the shared types at the root of a peer's document. */
  roots: (peer: PeerName) => { name: string; kind: string }[];
  /** What a peer's shared type holds. */
  shape: (peer: PeerName) => Shape;
  /** Cuts the relay: from now on each side keeps its changes to itself. */
  disconnect: () => void;
  /** Mends the relay and brings both sides up to date. */
  connect: () => void;
  /** The start and the end of the `index`-th node of a kind in a peer's editor. */
  range: (peer: PeerName, type: string, index: number) => { from: number; to: number };
  /** The transactions that changed a peer's document since the list was last cleared. */
  changes: (peer: PeerName) => ChangeRecord[];
  clearChanges: () => void;
  /** Marks a place in a peer's editor, as a decoration and as a mapped position. */
  placeMarker: (peer: PeerName, position: number) => void;
  marker: (peer: PeerName) => MarkerState;
  /** Writes shared content from outside the editors, below the element at `path`. */
  inject: (path: number[], index: number, nodes: RawNode[]) => void;
  /** Sets an attribute of the element at `path` from outside the editors. */
  injectAttribute: (path: number[], name: string, value: AttributeValue) => void;
  /** Deletes children of the element at `path` (or letters of the text there) from outside the editors. */
  injectDelete: (path: number[], index: number, length: number) => void;
  /** The messages of the content errors the editors reported. */
  contentErrors: string[];
  /** The names of the marks in the order of the editor's schema. */
  markOrder: () => string[];
}

declare global {
  interface Window {
    spike: SpikeApi;
  }
}
