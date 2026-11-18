/**
 * The file block: a file of the board or the wiki, on a line of its own in a
 * body, to download. The document holds the file's identifier and nothing
 * else (`schema.ts`); its name, size and kind are read from the file's record
 * where it is shown, as an image's address is made where it is drawn.
 *
 * A block copied out of one editor and pasted into another keeps its file
 * (`data-file-id`); an element without one is no file block here.
 *
 * In the editor the block is drawn as where a body is only shown
 * (file-view.ts). It is reached with the arrow keys, which select it, and
 * Enter then opens its file in the viewer, as a press of its name does,
 * when the viewer shows it, and saves it otherwise, as a press of its link
 * does. A press of the name is still the block's to select and drag.
 */
import { mergeAttributes, Node } from '@tiptap/core';
import { NodeSelection, Plugin } from '@tiptap/pm/state';

import { FILE_BLOCK_ATTRIBUTE, FILE_ID_ATTRIBUTE } from './drawing.ts';
import { drawFileBlock, pressFileOf } from './file-view.ts';

/** The events of the keys and of typing, which the name's button keeps from the editor. */
const BUTTON_KEYS = new Set([
  'keydown',
  'keypress',
  'keyup',
  'beforeinput',
  'input',
  'compositionstart',
  'compositionupdate',
  'compositionend',
]);

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

  addNodeView() {
    return ({ node, HTMLAttributes }) => {
      const dom = document.createElement('div');
      const attributes = mergeAttributes(HTMLAttributes, { [FILE_BLOCK_ATTRIBUTE]: '' });
      for (const [name, value] of Object.entries(attributes)) {
        if (value !== null && value !== undefined) dom.setAttribute(name, String(value));
      }
      drawFileBlock(dom);
      return {
        dom,
        // A press of the link is the link's, and the keys pressed on the name's button are the
        // button's (Enter and Space press it): the editor takes neither as selecting or editing.
        // A press of the button by a pointer is still the editor's, which selects or drags the block.
        stopEvent: (event) => {
          if (!(event.target instanceof Element)) return false;
          if (event.target.closest('a.doc-file-download')) return true;
          return (
            BUTTON_KEYS.has(event.type) && event.target.closest('button.doc-file-open') !== null
          );
        },
        // What is drawn into the block is no change of the document.
        ignoreMutation: () => true,
        // Another file is another block, drawn anew.
        update: (updated) =>
          updated.type === node.type && updated.attrs.fileId === node.attrs.fileId,
      };
    };
  },

  // Enter is taken from the key's own event, before the editor's keys: the editor leaves
  // Enter alone in Chrome on Android (it waits for what the keyboard does to the text),
  // and a selected block has no text to do it to.
  addProseMirrorPlugins() {
    const name = this.name;
    return [
      new Plugin({
        props: {
          handleDOMEvents: {
            keydown: (view, event) => {
              const plain = !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey;
              const { selection } = view.state;
              if (event.key !== 'Enter' || !plain || event.isComposing) return false;
              if (!(selection instanceof NodeSelection) || selection.node.type.name !== name) {
                return false;
              }
              const dom = view.nodeDOM(selection.from);
              if (dom instanceof HTMLElement) pressFileOf(dom);
              // Taken either way: Enter never replaces the selected block.
              event.preventDefault();
              return true;
            },
            // With the focus on a file's name, nothing is typed into the body: an edit comes to
            // the editor itself, and WebKit would make it where the caret was before.
            beforeinput: (view, event) => {
              const focused = view.dom.ownerDocument.activeElement;
              if (!(focused instanceof Element) || !focused.matches('button.doc-file-open')) {
                return false;
              }
              if (!view.dom.contains(focused)) return false;
              event.preventDefault();
              return true;
            },
          },
        },
      }),
    ];
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
