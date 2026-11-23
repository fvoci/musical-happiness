/**
 * The slash menu: typing `/` at the start of a line or after a space offers
 * what is put into a body, or done to it, by something other than typing:
 * a file where the body takes files (uploads.ts asks for it and puts its
 * block in), a table, and in a table what changes it (table-menu.ts). What
 * it offers depends on where the reader is; with nothing to offer, no menu.
 *
 * What follows the slash narrows the entries by their names in any of the
 * reader's words (`/파일`, `/file`). Not in a code block, whose text is
 * code, nor after anything but a space (`http:/`, `a/b`).
 */
import { Extension, type Editor } from '@tiptap/core';
import { PluginKey } from '@tiptap/pm/state';
import { Suggestion } from '@tiptap/suggestion';

import type { SuggestionEntry, SuggestionRenderer } from './extensions.ts';

/** One thing the menu offers. */
export interface SlashEntry {
  /** Tells it apart in the list (`data-value`). */
  value: string;
  /** What the list shows. */
  label: string;
  /** More names it is found by, in lower case. */
  words: string[];
  /** Does it, once the slash and what followed are gone; false when it cannot now. */
  run: (editor: Editor) => boolean;
}

export interface SlashMenuOptions {
  /** What the menu offers now, where the reader is in the editor; none: no menu. */
  entries: (editor: Editor) => SlashEntry[];
  render?: SuggestionRenderer;
}

/** The entries whose names have `query` in them, in the order given. */
export function matching(entries: SlashEntry[], query: string): SlashEntry[] {
  const wanted = query.trim().toLowerCase();
  return entries.filter(
    (entry) =>
      wanted === '' ||
      entry.label.toLowerCase().includes(wanted) ||
      entry.words.some((word) => word.includes(wanted)),
  );
}

export const SlashMenu = Extension.create<SlashMenuOptions>({
  name: 'slashMenu',

  addOptions() {
    return { entries: () => [] };
  },

  addProseMirrorPlugins() {
    const { options } = this;
    return [
      Suggestion<SuggestionEntry, SuggestionEntry>({
        editor: this.editor,
        pluginKey: new PluginKey('slashMenu'),
        char: '/',
        allowedPrefixes: [' '],
        allow: ({ editor, state, range }) =>
          editor.isEditable &&
          state.doc.resolve(range.from).parent.type.spec.code !== true &&
          options.entries(editor).length > 0,
        items: ({ query, editor }) =>
          matching(options.entries(editor), query).map(({ value, label }) => ({ value, label })),
        command: ({ editor, range, props }) => {
          const entry = options.entries(editor).find((each) => each.value === props.value);
          editor.chain().focus().deleteRange(range).run();
          entry?.run(editor);
        },
        ...(options.render ? { render: options.render } : {}),
      }),
    ];
  },
});
