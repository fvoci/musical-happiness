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
 * its block, does not move the image elsewhere. The image is not put in,
 * and that is said, when the block it was going into was deleted or made
 * anew (by anybody; by the reader, when the spot itself is taken away),
 * when the reader may no longer write, and when the editor takes no change
 * for now (content it cannot draw). An editor read only for a moment (the
 * connection away) takes it: the room sends it later.
 */
import { Extension } from '@tiptap/core';
import { Plugin, PluginKey, type EditorState, type Transaction } from '@tiptap/pm/state';
import { Decoration, DecorationSet, type EditorView } from '@tiptap/pm/view';

import {
  fromShared,
  holdPlace,
  mayWrite,
  onShared,
  placeNow,
  type HeldPlace,
} from './shared-place.ts';

/**
 * How an upload ended for the reader: put in, failed (or given up), its
 * place deleted, or the editor no longer taking it in.
 */
export type UploadOutcome = 'done' | 'failed' | 'gone' | 'refused';

export interface ImageUploadOptions {
  /** Uploads a file and gives its identifier. Undefined: images cannot be added. */
  upload: () => ((file: File) => Promise<string>) | undefined;
  /** What the placeholder says to a screen reader. */
  uploadingLabel: () => string;
  /**
   * How many uploads are under way, whenever that changes, and how the one
   * that just ended did. Also once the editor is gone (`destroyed`): an
   * image that could not be put in is said by whoever is still there.
   */
  onProgress: (pending: number, outcome: UploadOutcome | undefined, destroyed: boolean) => void;
}

type Change = { add: { id: number; position: number; label: string } } | { remove: number };

/** Where an image goes once it is up. */
interface Placeholder {
  id: number;
  pos: number;
  /** The place in the shared document, in a body edited together. */
  held: HeldPlace | undefined;
  /** Moved by the reader's own change: held again once it is in the shared document. */
  stale: boolean;
  label: string;
  /** Its block was deleted: the image has nowhere to go. */
  gone: boolean;
}

const key = new PluginKey<Placeholder[]>('imageUpload');

/** Whether a transaction is this extension's own (a placeholder added or taken away, an image put in). */
export const fromUpload = (tr: Transaction): boolean => tr.getMeta(key) !== undefined;

/** Where a placeholder is after a transaction. */
function follow(placeholder: Placeholder, tr: Transaction, before: EditorState): Placeholder {
  if (placeholder.gone) return placeholder;
  if (placeholder.held && fromShared(tr)) {
    const now = placeNow(before, placeholder.held, tr.doc);
    if (now === null) return { ...placeholder, gone: true };
    if (now !== undefined) return { ...placeholder, pos: now };
  }
  if (!tr.docChanged) return placeholder;
  // The reader's own change took away the spot itself (its line deleted, or text around it
  // replaced): the image has nowhere to go, as when somebody else deleted it.
  const mapped = tr.mapping.mapResult(placeholder.pos);
  if (mapped.deletedAcross) return { ...placeholder, gone: true };
  return { ...placeholder, pos: mapped.pos, stale: true };
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

/** Puts the image of an upload that ended where its placeholder is now, if it may. */
function putIn(view: EditorView, id: number, fileId: string | undefined): UploadOutcome {
  const placeholder = key.getState(view.state)?.find((each) => each.id === id);
  const at = placeholder ? placeOf(view.state, placeholder) : null;
  const remove = (): void => {
    view.dispatch(view.state.tr.setMeta(key, { remove: id } satisfies Change));
  };
  const image = view.state.schema.nodes.image;
  if (fileId === undefined || fileId === '' || !image) {
    remove();
    return 'failed';
  }
  if (at === null || !mayWrite(view)) {
    remove();
    return at === null ? 'gone' : 'refused';
  }
  const before = view.state.doc;
  const transaction = view.state.tr.setMeta(key, { remove: id } satisfies Change);
  view.dispatch(transaction.replaceRangeWith(at, at, image.create({ fileId })));
  if (view.state.doc !== before) return 'done';
  // Not taken (the editor takes no change while it holds content it cannot draw).
  remove();
  return 'refused';
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
        options.onProgress(pending, undefined, false);
        const finish = (fileId: string | undefined): void => {
          pending -= 1;
          if (view.isDestroyed) {
            // The editor was made anew (another document of the room): the image has no place.
            options.onProgress(pending, fileId === undefined ? 'failed' : 'gone', true);
            return;
          }
          options.onProgress(pending, putIn(view, id, fileId), false);
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
          apply(transaction, list, before) {
            let next = list.map((placeholder) => follow(placeholder, transaction, before));
            const change = transaction.getMeta(key) as Change | undefined;
            if (change && 'add' in change) {
              const { id, position, label } = change.add;
              const pos = Math.min(position, transaction.doc.content.size);
              next = [...next, { id, pos, held: undefined, stale: true, label, gone: false }];
            } else if (change && 'remove' in change) {
              next = next.filter((placeholder) => placeholder.id !== change.remove);
            }
            return next;
          },
        },
        // Held once the change that put it there is in the shared document (shared-place.ts):
        // after the binding writes it, or at once when nothing was to be written (the body empty).
        view: (view) => {
          const hold = (): void => {
            for (const placeholder of key.getState(view.state) ?? []) {
              if (!placeholder.stale || placeholder.gone) continue;
              placeholder.held = holdPlace(view.state, placeholder.pos);
              placeholder.stale = false;
            }
          };
          return {
            update: (_view, before) => {
              if (view.state.doc === before.doc) hold();
            },
            destroy: onShared(view, hold),
          };
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
