/**
 * Where the reader's caret goes when the body's editor is made: where they
 * tapped the body while it was still shown read-only (the room's document
 * had not arrived), or, when the editor is made again for another document
 * of the room while the reader was in it, where the caret was in the one
 * before, with the focus. Positions past the end of the new document are
 * taken in.
 *
 * Only a press on the read-only view that did not move (a tap, a click, not
 * a scroll) counts. Nothing here imports the editor's code: this is part of
 * the first screen, and the editor is not (editor-chunk.svelte.ts).
 */
import type { Editor } from '@tiptap/core';

/** How long a tap on the read-only body still counts once the editor is there. */
const PRESS_KEPT = 5000;
/** How far a press may move, in pixels, and still be a tap. */
const SLOP = 8;

interface Held {
  from: number;
  to: number;
  focused: boolean;
}

export interface CaretKeeper {
  /** Watches the taps on the read-only view of the body: an attachment. */
  watch: (view: HTMLElement) => () => void;
  /** An editor was made: the caret is put where it belongs, if anywhere. */
  take: (editor: Editor, writable: boolean) => void;
}

export function keepCaret(): CaretKeeper {
  let press: { x: number; y: number; at: number } | undefined;
  let held: Held | undefined;

  const remember = (editor: Editor): void => {
    if (editor.isDestroyed) return;
    const { from, to } = editor.state.selection;
    held = { from, to, focused: editor.isFocused };
  };

  return {
    watch(view) {
      let down: { x: number; y: number } | undefined;
      const pressed = (event: PointerEvent): void => {
        down = { x: event.clientX, y: event.clientY };
      };
      const released = (event: PointerEvent): void => {
        if (!down) return;
        const moved = Math.hypot(event.clientX - down.x, event.clientY - down.y);
        if (moved <= SLOP) press = { x: event.clientX, y: event.clientY, at: Date.now() };
        down = undefined;
      };
      const cancelled = (): void => {
        down = undefined;
      };
      view.addEventListener('pointerdown', pressed);
      view.addEventListener('pointerup', released);
      view.addEventListener('pointercancel', cancelled);
      return () => {
        view.removeEventListener('pointerdown', pressed);
        view.removeEventListener('pointerup', released);
        view.removeEventListener('pointercancel', cancelled);
      };
    },

    take(editor, writable) {
      const before = held;
      const pressed = press;
      held = undefined;
      press = undefined;
      editor.on('transaction', () => {
        remember(editor);
      });
      editor.on('focus', () => {
        remember(editor);
      });
      // Turned read-only (the document is sent again) is no leaving: the focus is kept for the next editor.
      editor.on('blur', () => {
        if (editor.isEditable) remember(editor);
      });
      if (!writable) return;
      const size = editor.state.doc.content.size;
      const clamp = (at: number): number => Math.max(0, Math.min(at, size));
      // The caret of an editor the reader was not in is nobody's: the new editor's own stands.
      if (before?.focused) {
        editor.commands.setTextSelection({ from: clamp(before.from), to: clamp(before.to) });
        editor.commands.focus();
        return;
      }
      if (!pressed || Date.now() - pressed.at > PRESS_KEPT) return;
      const found = editor.view.posAtCoords({ left: pressed.x, top: pressed.y });
      if (!found) return;
      editor.commands.setTextSelection(clamp(found.pos));
      editor.commands.focus();
    },
  };
}
