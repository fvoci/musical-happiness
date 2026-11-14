/**
 * A reference in a body: the identifier of a card or a page of the
 * workspace, and nothing else. What it names, and whether the reader may see
 * it, is asked where the node is drawn (reference-chip.ts), so a title is
 * never part of a document: a renamed card shows under its new name, a
 * reader who may not see it is shown no name at all, and what is copied out
 * of the editor carries the identifier alone.
 *
 * A reference is put in three ways: by choosing from what `[[` offers, by
 * pasting the address of a card or a page of the workspace that the reader
 * may see, and by pasting what was copied out of a body.
 */
import { mergeAttributes, Node } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { Suggestion, type SuggestionMatch, type Trigger } from '@tiptap/suggestion';

import { REFERENCE_CLASS, REFERENCE_ID_ATTRIBUTE } from './drawing.ts';
import type { SuggestionEntry, SuggestionRenderer } from './extensions.ts';
import { drawReference, openClickedReference, type ReferenceDrawing } from './reference-chip.ts';
import { isBlockId } from './schema.ts';

export interface ReferenceOptions {
  /** What the references are drawn from, as it is now (reference-chip.ts). */
  drawing: () => ReferenceDrawing | undefined;
  /** What may be named whose title matches what was typed after `[[`. */
  search: (query: string) => SuggestionEntry[] | Promise<SuggestionEntry[]>;
  /** The identifier an address names, if it is the address of a card or a page of this workspace. */
  named: (address: string) => string | undefined;
  /** Whether the reader may see what the identifier names: only then is a pasted address made a reference. */
  seen: (id: string) => Promise<boolean>;
  render?: SuggestionRenderer;
}

/** The longest title that is looked for after `[[`. */
const LONGEST_QUERY = 80;

/**
 * What is being typed after `[[` at the cursor, if anything is: up to the
 * cursor, spaces included (a title has them), within the line. A `]` ends
 * it, and so does a node that is not text. Not in code.
 */
export function referenceMatch({ $position }: Trigger): SuggestionMatch {
  if ($position.parent.type.spec.code) return null;
  if ($position.marks().some((mark) => mark.type.spec.code)) return null;
  const before = $position.parent.textBetween(0, $position.parentOffset, undefined, '￼');
  const index = before.lastIndexOf('[[');
  if (index === -1) return null;
  const query = before.slice(index + 2);
  if (query.length > LONGEST_QUERY || /[\]￼\n]/.test(query)) return null;
  return {
    range: { from: $position.start() + index, to: $position.pos },
    query,
    text: `[[${query}`,
  };
}

export const Reference = Node.create<ReferenceOptions>({
  name: 'reference',
  group: 'inline',
  inline: true,
  atom: true,

  addOptions() {
    return {
      drawing: () => undefined,
      search: () => [],
      named: () => undefined,
      seen: () => Promise.resolve(false),
    };
  },

  addAttributes() {
    return {
      id: {
        default: null,
        parseHTML: (element: HTMLElement) => element.getAttribute(REFERENCE_ID_ATTRIBUTE),
        renderHTML: (attrs: { id?: string | null }) => ({ [REFERENCE_ID_ATTRIBUTE]: attrs.id }),
      },
    };
  },

  parseHTML() {
    // A reference that names nothing (pasted from elsewhere, or from a view a
    // reader by a link got) is not one: the server would refuse the body.
    return [
      {
        tag: `span[data-reference][${REFERENCE_ID_ATTRIBUTE}]`,
        getAttrs: (element: HTMLElement) =>
          isBlockId(element.getAttribute(REFERENCE_ID_ATTRIBUTE)) ? null : false,
      },
    ];
  },

  // What is copied out and what `getHTML` gives: the identifier, and no title.
  renderHTML({ HTMLAttributes }) {
    return [
      'span',
      mergeAttributes(HTMLAttributes, { 'data-reference': '', class: REFERENCE_CLASS }),
    ];
  },

  // The title is not the document's: copied as text, a reference is nothing.
  renderText() {
    return '';
  },

  // What the reader sees: the same element, with the title drawn into it.
  addNodeView() {
    return ({ HTMLAttributes }) => {
      const dom = document.createElement('span');
      const attributes = mergeAttributes(HTMLAttributes, {
        'data-reference': '',
        class: REFERENCE_CLASS,
      });
      for (const [name, value] of Object.entries(attributes)) dom.setAttribute(name, String(value));
      drawReference(dom, this.options.drawing());
      dom.addEventListener('click', (event) => {
        openClickedReference(event, this.options.drawing());
      });
      // No part of it is the document's: what is drawn inside is no change of it.
      return { dom };
    };
  },

  addProseMirrorPlugins() {
    const { editor, name, options } = this;
    const put = (id: string) => [
      { type: name, attrs: { id } },
      { type: 'text', text: ' ' },
    ];
    return [
      Suggestion<SuggestionEntry, SuggestionEntry>({
        editor,
        pluginKey: new PluginKey('referenceSuggestion'),
        char: '[[',
        findSuggestionMatch: referenceMatch,
        items: ({ query }) => options.search(query),
        command: ({ editor: current, range, props }) => {
          current.chain().focus().insertContentAt(range, put(props.value)).run();
        },
        ...(options.render ? { render: options.render } : {}),
      }),
      new Plugin({
        key: new PluginKey('referencePaste'),
        props: {
          handlePaste: (view, event) => {
            const text = event.clipboardData?.getData('text/plain').trim() ?? '';
            const { selection } = view.state;
            // Over a selection an address is the link of what is selected; in code it is text.
            if (!selection.empty || selection.$from.parent.type.spec.code) return false;
            const id = text === '' || /\s/.test(text) ? undefined : options.named(text);
            if (id === undefined || !isBlockId(id)) return false;
            const answered = options.seen(id).catch(() => false);
            void answered.then((seen) => {
              if (editor.isDestroyed) return;
              if (seen) editor.chain().focus().insertContent(put(id)).run();
              // What the reader may not see stays the address it was (pasted with no
              // clipboard of its own, so it does not come here again).
              else view.pasteText(text);
            });
            return true;
          },
        },
      }),
    ];
  },
});
