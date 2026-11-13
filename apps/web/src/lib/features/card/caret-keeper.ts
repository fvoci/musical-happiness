/**
 * Where the reader's caret goes when the body's editor is made: where they
 * pressed on the body while it was still shown read-only (the room's
 * document had not arrived), or, when the editor is made again for another
 * document of the room while the reader was in it, where the caret was in
 * the one before, with the focus. Positions past the end of the new
 * document are taken in.
 */
import type { Editor } from '@tiptap/core';
import { TextSelection } from '@tiptap/pm/state';

/** How long a press on the read-only body still counts once the editor is there. */
const PRESS_KEPT = 5000;

interface Held {
  from: number;
  to: number;
  focused: boolean;
}

export interface CaretKeeper {
  /** Watches the presses on the body: an attachment. */
  watch: (section: HTMLElement) => () => void;
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
    watch(section) {
      const pressed = (event: PointerEvent): void => {
        press = { x: event.clientX, y: event.clientY, at: Date.now() };
      };
      section.addEventListener('pointerdown', pressed, { capture: true });
      return () => {
        section.removeEventListener('pointerdown', pressed, { capture: true });
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
      editor.on('blur', () => {
        remember(editor);
      });
      if (!writable) return;
      const size = editor.state.doc.content.size;
      // The caret of an editor the reader was not in is nobody's: the new editor's own stands.
      if (before?.focused) {
        const clamp = (at: number): number => Math.max(0, Math.min(at, size));
        const selection = TextSelection.between(
          editor.state.doc.resolve(clamp(before.from)),
          editor.state.doc.resolve(clamp(before.to)),
        );
        editor.view.dispatch(editor.state.tr.setSelection(selection));
        editor.view.focus();
        return;
      }
      if (!pressed || Date.now() - pressed.at > PRESS_KEPT) return;
      const found = editor.view.posAtCoords({ left: pressed.x, top: pressed.y });
      if (!found) return;
      const selection = TextSelection.near(editor.state.doc.resolve(found.pos));
      editor.view.dispatch(editor.state.tr.setSelection(selection));
      editor.view.focus();
    },
  };
}
