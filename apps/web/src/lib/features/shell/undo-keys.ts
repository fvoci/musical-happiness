/**
 * The keys for undo and redo: Ctrl+Z or ⌘Z, and with Shift for redo.
 *
 * Inside a field they are left alone: a field has an undo of its own for
 * what was typed, and taking the keys there would undo a change of the board
 * while the reader meant a word.
 */

export type UndoKey = 'undo' | 'redo';

const FIELDS = 'input, textarea, select, [contenteditable]:not([contenteditable="false"])';

/** Whether an event started in something the reader types into. */
export function inField(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(FIELDS) !== null;
}

type KeyPress = Pick<
  KeyboardEvent,
  'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey' | 'target' | 'defaultPrevented'
>;

/** What a key press asks for, if it is one of the two. */
export function undoKey(event: KeyPress): UndoKey | undefined {
  if (event.defaultPrevented || event.altKey) return undefined;
  if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 'z') return undefined;
  if (inField(event.target)) return undefined;
  return event.shiftKey ? 'redo' : 'undo';
}
