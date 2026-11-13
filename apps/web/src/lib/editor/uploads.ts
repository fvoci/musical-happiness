/**
 * Images that are pasted or dropped into the editor as files.
 *
 * A document names an image by the identifier of an uploaded file, and a
 * file has an identifier only once its upload is complete (files.ts). So a
 * pasted file is handed to the caller's `upload`, a placeholder marks where
 * the image will go, and the image node is inserted when the identifier
 * arrives. The placeholder is a decoration, no part of the document: nothing
 * about an upload in progress is ever stored. Without an `upload` function
 * the editor takes no image files at all.
 *
 * In a body edited together the place is held in the shared document
 * (shared-place.ts), so that what others write meanwhile, before it or in
 * its block, does not move the image elsewhere. When the block it was going
 * into is deleted (by anybody) the image is not put in, and that is said.
 */
import { Extension } from '@tiptap/core';
import { Plugin, PluginKey, type EditorState, type Transaction } from '@tiptap/pm/state';
import { Decoration, DecorationSet, type EditorView } from '@tiptap/pm/view';

import { fromShared, holdPlace, placeNow, type HeldPlace } from './shared-place.ts';

/** How an upload ended for the reader: put in, failed (or given up), or its place was deleted. */
export type UploadOutcome = 'done' | 'failed' | 'gone';

export interface ImageUploadOptions {
  /** Uploads a file and gives its identifier. Undefined: images cannot be added. */
  upload: () => ((file: File) => Promise<string>) | undefined;
  /** What the placeholder says to a screen reader. */
  uploadingLabel: () => string;
  /** How many uploads are under way, whenever that changes, and how the one that just ended did. */
  onProgress: (pending: number, outcome: UploadOutcome | undefined) => void;
}

type Change = { add: { id: number; position: number; label: string } } | { remove: number };

/** Where an image goes once it is up. */
interface Placeholder {
  id: number;
  pos: number;
  /** The place in the shared document, in a body edited together. */
  held: HeldPlace | undefined;
  label: string;
  /** Its block was deleted: the image has nowhere to go. */
  gone: boolean;
}

const key = new PluginKey<Placeholder[]>('imageUpload');

/** Where a placeholder is after a transaction. */
function follow(placeholder: Placeholder, tr: Transaction, state: EditorState): Placeholder {
  if (placeholder.gone) return placeholder;
  if (placeholder.held && fromShared(tr)) {
    const now = placeNow(state, placeholder.held);
    if (now === null) return { ...placeholder, gone: true };
    if (now !== undefined) return { ...placeholder, pos: now };
  }
  return { ...placeholder, pos: tr.mapping.map(placeholder.pos) };
}

/** Where the image of a placeholder goes now: a position, `null` when its block is gone. */
function placeOf(state: EditorState, placeholder: Placeholder): number | null {
  if (placeholder.gone) return null;
  if (!placeholder.held) return placeholder.pos;
  return placeNow(state, placeholder.held) ?? null;
}

function widget(placeholder: Placeholder, size: number): Decoration {
  const { id, label } = placeholder;
  return Decoration.widget(
    Math.min(placeholder.pos, size),
    (view) => {
      const element = view.dom.ownerDocument.createElement('span');
      element.setAttribute('class', 'doc-upload');
      element.setAttribute('role', 'img');
      element.setAttribute('aria-label', label);
      return element;
    },
    { id, key: `doc-upload-${String(id)}` },
  );
}

/**
 * The kinds of picture taken. The server serves every file as bytes of no
 * kind (`application/octet-stream`, `nosniff`); a browser still draws these
 * by what they are, but not an SVG, nor kinds it does not draw at all.
 */
const IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp', 'image/avif']);

const imagesIn = (files: FileList | null | undefined): File[] =>
  Array.from(files ?? []).filter((file) => IMAGE_TYPES.has(file.type));

export const ImageUpload = Extension.create<ImageUploadOptions>({
  name: 'imageUpload',

  addOptions() {
    return { upload: () => undefined, uploadingLabel: () => '', onProgress: () => undefined };
  },

  addProseMirrorPlugins() {
    const { options } = this;
    let serial = 0;
    let pending = 0;

    function start(view: EditorView, files: File[], position: number): boolean {
      const upload = options.upload();
      if (!upload || files.length === 0 || !view.editable) return false;
      for (const file of files) {
        serial += 1;
        const id = serial;
        pending += 1;
        view.dispatch(
          view.state.tr.setMeta(key, {
            add: { id, position, label: options.uploadingLabel() },
          } satisfies Change),
        );
        options.onProgress(pending, undefined);
        const finish = (uploaded: string | undefined): void => {
          // Put in even while the editor is read only for now (the connection away): the room
          // sends it when it can, or says it was lost; a reader who may no longer write is refused.
          const fileId = uploaded;
          pending -= 1;
          if (view.isDestroyed) {
            options.onProgress(pending, fileId === undefined ? 'failed' : 'done');
            return;
          }
          // Where the placeholder is now; it has moved with whatever was written meanwhile.
          const placeholder = key.getState(view.state)?.find((each) => each.id === id);
          const at = placeholder ? placeOf(view.state, placeholder) : null;
          const transaction = view.state.tr.setMeta(key, { remove: id } satisfies Change);
          const image = view.state.schema.nodes.image;
          const placed = fileId !== undefined && fileId !== '' && at !== null && image;
          if (placed) transaction.replaceRangeWith(at, at, image.create({ fileId }));
          view.dispatch(transaction);
          if (fileId === undefined) options.onProgress(pending, 'failed');
          else options.onProgress(pending, placed ? 'done' : 'gone');
        };
        upload(file).then(finish, () => {
          finish(undefined);
        });
      }
      return true;
    }

    return [
      new Plugin<Placeholder[]>({
        key,
        state: {
          init: () => [],
          apply(transaction, list, _before, state) {
            let next = list.map((placeholder) => follow(placeholder, transaction, state));
            const change = transaction.getMeta(key) as Change | undefined;
            if (change && 'add' in change) {
              const { id, position, label } = change.add;
              const pos = Math.min(position, transaction.doc.content.size);
              next = [...next, { id, pos, held: holdPlace(state, pos, true), label, gone: false }];
            } else if (change && 'remove' in change) {
              next = next.filter((placeholder) => placeholder.id !== change.remove);
            }
            return next;
          },
        },
        props: {
          decorations: (state) => {
            const list = (key.getState(state) ?? []).filter((placeholder) => !placeholder.gone);
            if (list.length === 0) return DecorationSet.empty;
            const size = state.doc.content.size;
            return DecorationSet.create(
              state.doc,
              list.map((placeholder) => widget(placeholder, size)),
            );
          },
          handlePaste: (view, event) => {
            const files = imagesIn(event.clipboardData?.files);
            if (!start(view, files, view.state.selection.from)) return false;
            event.preventDefault();
            return true;
          },
          handleDrop: (view, event) => {
            const files = imagesIn(event.dataTransfer?.files);
            const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos;
            if (!start(view, files, at ?? view.state.selection.from)) return false;
            event.preventDefault();
            return true;
          },
        },
      }),
    ];
  },
});
