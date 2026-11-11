/**
 * What the editor of a body (BodyEditor.svelte) takes and gives. Kept in a
 * file of its own so that a caller can name these types without importing
 * the component, which it fetches only when a body is first edited.
 */
import type { Editor } from '@tiptap/core';
import type { Snippet } from 'svelte';

import type { DocNode } from './schema.ts';

/** What a toolbar gets: the editor to run commands on, and a count that changes with its state. */
export interface BodyEditorTools {
  editor: Editor;
  /** Goes up with every change of the document or the selection; read it to stay current. */
  changes: number;
}

export interface BodyEditorProps {
  /** The stored document, `null` for an empty body. Read on creation and when `revision` changes. */
  doc: DocNode | null;
  /** Raise it to have `doc` read again after a change from outside. Unless it is raised, what is being written is never overwritten. */
  revision?: number;
  /** True unless given. */
  editable?: boolean;
  /** The people who can be mentioned. */
  users?: readonly { id: string; name: string }[];
  /** The name of a user a document mentions; undefined when it is not known. */
  resolveUser?: (userId: string) => string | undefined;
  /** Every change, as the document that would be stored; `null` for an empty body. Debouncing is the caller's business. */
  onChange?: (doc: DocNode | null) => void;
  onFocus?: () => void;
  onBlur?: () => void;
  /** Uploads a pasted or dropped image and gives its file identifier. Without it images cannot be added. */
  uploadImage?: (file: File) => Promise<string>;
  placeholder?: string;
  /** Classes for the text field itself, next to `doc-body`. */
  class?: string;

  // Beyond the agreed shape, all optional:

  /** The accessible name of the text field; "Body" in the app's language unless given. */
  label?: string;
  /** Drawn above the text, inside the editor's frame: the caller's formatting buttons. */
  toolbar?: Snippet<[BodyEditorTools]>;
  /**
   * Called when `doc` has been read, on creation and after every `revision`:
   * with what the editor would store for it, or with `undefined` when the
   * document was refused and the editor is read-only. For a valid document
   * that is the document itself, but for what the editor settles its own
   * way (the marks of a text come in the editor's order).
   */
  onLoad?: (stored: DocNode | null | undefined) => void;
}
