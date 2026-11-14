/**
 * A reference in a body: the identifier of a card or a page of the
 * workspace, and nothing else. What it names, and whether the reader may see
 * it, is asked where the node is drawn, so a title is never part of a
 * document: a renamed card shows under its new name, and a reader who may
 * not see it is shown no name at all.
 */
import { mergeAttributes, Node } from '@tiptap/core';

import { REFERENCE_CLASS, REFERENCE_ID_ATTRIBUTE } from './drawing.ts';
import { isBlockId } from './schema.ts';

export const Reference = Node.create({
  name: 'reference',
  group: 'inline',
  inline: true,
  atom: true,

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
});
