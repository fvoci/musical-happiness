/**
 * A place in a body edited together, held across what others write.
 *
 * A position in the editor is a number: what others write before it moves
 * the place, and a change that comes from the shared document replaces the
 * blocks it changed in the editor's document, so the editor's own mapping
 * loses any position inside them. A place held for later (an image going
 * up, a formula or a diagram being edited) is held here as a relative
 * position of the shared document as well, which follows its spot through
 * every change, and is read back as a number when it is needed.
 *
 * In an editor that is not bound to a shared document there is nothing to
 * hold: the editor's mapping is all there is (`undefined` throughout).
 */
import {
  absolutePositionToRelativePosition,
  relativePositionToAbsolutePosition,
  ySyncPluginKey,
} from '@tiptap/y-tiptap';
import type { EditorState, Transaction } from '@tiptap/pm/state';
import * as Y from 'yjs';

export interface HeldPlace {
  /** The place itself. */
  at: Y.RelativePosition;
  /**
   * For a place in a line of text: the letter (or inline node) before it,
   * and the spot after its block. The letter is followed first; when it was
   * taken away (deleted, or moved into another block, which a split does by
   * writing the text anew) the place is taken to be the end of its line, and
   * found after the block that is there now.
   */
  before?: Y.RelativePosition;
  after?: Y.RelativePosition;
}

/**
 * Holds the place at `pos` in the shared document; undefined in an editor
 * bound to none. With `line`, a place in a line of text is held also by the
 * letter before it and the end of its block (`HeldPlace`): for what goes
 * after a line (an image), not for a node that has to be found itself.
 */
export function holdPlace(state: EditorState, pos: number, line = false): HeldPlace | undefined {
  const sync = ySyncPluginKey.getState(state);
  if (!sync?.binding) return undefined;
  const { mapping } = sync.binding;
  try {
    const at = absolutePositionToRelativePosition(pos, sync.type, mapping);
    const $pos = state.doc.resolve(pos);
    if (!line || !$pos.parent.isTextblock || $pos.parentOffset === 0) return { at };
    return {
      at,
      before: absolutePositionToRelativePosition(pos - 1, sync.type, mapping),
      after: absolutePositionToRelativePosition($pos.after(), sync.type, mapping),
    };
  } catch {
    return undefined;
  }
}

/** Whether the item a relative position names was deleted. */
function itemDeleted(doc: Y.Doc, position: Y.RelativePosition): boolean {
  if (!position.item) return false;
  try {
    return Y.getItem(doc.store, position.item).deleted;
  } catch {
    return true;
  }
}

/** Whether a shared type, or one it is in, was deleted. */
function deleted(type: Y.AbstractType<unknown> | null): boolean {
  for (let at = type; at?._item; at = at._item.parent as Y.AbstractType<unknown> | null) {
    if (at._item.deleted) return true;
  }
  return false;
}

/**
 * Where a held place is now: a position, or `null` when the block it was in
 * was deleted (by anybody). Undefined in an editor bound to no shared document.
 */
export function placeNow(state: EditorState, held: HeldPlace): number | null | undefined {
  const sync = ySyncPluginKey.getState(state);
  if (!sync?.binding) return undefined;
  const { doc, type, binding } = sync;
  const absolute = Y.createAbsolutePositionFromRelativePosition(held.at, doc);
  if (!absolute || deleted(absolute.type)) return null;
  const read = (position: Y.RelativePosition): number | null =>
    relativePositionToAbsolutePosition(doc, type, position, binding.mapping);
  let pos: number | null;
  if (held.before && !itemDeleted(doc, held.before)) {
    const before = read(held.before);
    pos = before === null ? null : before + 1;
  } else if (held.after && held.before) pos = read(held.after);
  else pos = read(held.at);
  return pos === null ? null : Math.min(pos, state.doc.content.size);
}

/** Whether a transaction is a change of the shared document (another's, or the shared undo). */
export function fromShared(tr: Transaction): boolean {
  const meta = tr.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined;
  return meta?.isChangeOrigin === true;
}
