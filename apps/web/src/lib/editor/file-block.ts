/**
 * The file block: a file of the board or the wiki, on a line of its own in a
 * body, to download. The document holds the file's identifier and nothing
 * else (`schema.ts`); its name, size and kind are read from the file's record
 * where it is shown, as an image's address is made where it is drawn.
 *
 * A block copied out of one editor and pasted into another keeps its file
 * (`data-file-id`); an element without one is no file block here.
 */
import { mergeAttributes, Node } from '@tiptap/core';

import { FILE_BLOCK_ATTRIBUTE, FILE_ID_ATTRIBUTE } from './drawing.ts';

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    fileBlock: {
      /** Inserts a file block that names an uploaded file. */
      setFileBlock: (attrs: { fileId: string }) => ReturnType;
    };
  }
}

export const FileBlock = Node.create({
  name: 'file',
  group: 'block',
  atom: true,
  draggable: true,
  selectable: true,

  addAttributes() {
    return {
      fileId: {
        default: null,
        parseHTML: (element: HTMLElement) => element.getAttribute(FILE_ID_ATTRIBUTE),
        renderHTML: (attrs: { fileId?: string | null }) => ({ [FILE_ID_ATTRIBUTE]: attrs.fileId }),
      },
    };
  },

  parseHTML() {
    return [{ tag: `div[${FILE_BLOCK_ATTRIBUTE}][${FILE_ID_ATTRIBUTE}]` }];
  },

  renderHTML({ HTMLAttributes }) {
    return ['div', mergeAttributes(HTMLAttributes, { [FILE_BLOCK_ATTRIBUTE]: '' })];
  },

  addCommands() {
    return {
      setFileBlock:
        (attrs) =>
        ({ commands }) =>
          commands.insertContent({ type: this.name, attrs }),
    };
  },
});
