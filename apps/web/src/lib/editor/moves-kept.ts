/**
 * A move of the reader's caret made by a key the browser handles itself (an
 * arrow, Home, End) is in the page before the editor knows of it: the
 * browser says so a moment later. Whatever comes from the others in that
 * moment, a caret of theirs to draw or what they wrote, has the editor write
 * the selection it still holds into the page, and the move is taken back.
 *
 * So before such a change is applied the selection of the page is read, and
 * the change carries it: as it is where the document stays the same, and
 * through the change where the others wrote (`placeAfter`). A selection
 * within what they changed is left to the binding, which puts it back by
 * what it stood next to.
 */
import { Extension } from '@tiptap/core';
import type { Node } from '@tiptap/pm/model';
import { TextSelection, type Selection, type Transaction } from '@tiptap/pm/state';
import type { EditorView } from '@tiptap/pm/view';
import { yCursorPluginKey, ySyncPluginKey } from '@tiptap/y-tiptap';

/**
 * Where a position of `before` is in `after`, the two alike but for one
 * stretch: itself ahead of the stretch, moved by what the stretch grew
 * behind it, and undefined within it.
 */
export function placeAfter(before: Node, after: Node, position: number): number | undefined {
  const start = before.content.findDiffStart(after.content);
  if (start === null || position <= start) return position;
  const end = before.content.findDiffEnd(after.content);
  if (!end) return position;
  // Where the two ends reach past the start (the same letters on both sides), the stretch ends later.
  const short = Math.max(0, start - Math.min(end.a, end.b));
  return position >= end.a + short ? position + end.b - end.a : undefined;
}

/** The part of the view that reads the page: no public name, so each member may be missing. */
interface Reader {
  flush?: () => void;
  pendingRecords?: () => unknown[];
}

/**
 * Has the editor read the selection of the page now, where it waits to be
 * told of it. Not while something written is unread as well: reading that
 * here would change the document under the change about to be applied.
 */
function readSelection(view: EditorView): void {
  const reader = (view as unknown as { domObserver?: Reader }).domObserver;
  if (!reader?.flush || !reader.pendingRecords || reader.pendingRecords().length > 0) return;
  reader.flush();
}

/** Whether a transaction is what the others did: a caret of theirs to draw, or a change of the shared document. */
function fromOthers(transaction: Transaction): boolean {
  const drawn = transaction.getMeta(yCursorPluginKey) as { awarenessUpdated?: boolean } | undefined;
  if (drawn?.awarenessUpdated) return !transaction.docChanged && !transaction.selectionSet;
  const shared = transaction.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined;
  return shared?.isChangeOrigin === true;
}

/** Has a transaction of the others end with the selection the reader moved to, where it has a place there. */
function carry(transaction: Transaction, selection: Selection): void {
  if (!transaction.docChanged) {
    transaction.setSelection(selection);
    return;
  }
  if (!(selection instanceof TextSelection)) return;
  const anchor = placeAfter(transaction.before, transaction.doc, selection.anchor);
  const head = placeAfter(transaction.before, transaction.doc, selection.head);
  if (anchor === undefined || head === undefined) return;
  transaction.setSelection(TextSelection.create(transaction.doc, anchor, head));
}

/** The extension that keeps a move of the reader's caret when a change of the others comes right behind it. */
export const MovesKept = Extension.create({
  name: 'movesKept',

  dispatchTransaction({ transaction, next }) {
    const { view } = this.editor;
    if (fromOthers(transaction) && !view.composing) {
      const held = view.state.selection;
      readSelection(view);
      const { selection, doc } = view.state;
      if (!selection.eq(held) && doc === transaction.before) carry(transaction, selection);
    }
    next(transaction);
  },
});
