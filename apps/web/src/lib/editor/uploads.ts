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
 */
import { Extension } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { Decoration, DecorationSet, type EditorView } from '@tiptap/pm/view';

export interface ImageUploadOptions {
  /** Uploads a file and gives its identifier. Undefined: images cannot be added. */
  upload: () => ((file: File) => Promise<string>) | undefined;
  /** What the placeholder says to a screen reader. */
  uploadingLabel: () => string;
  /** How many uploads are under way, whenever that changes; `failed` when one just failed. */
  onProgress: (pending: number, failed: boolean) => void;
}

type Change = { add: { id: number; position: number; label: string } } | { remove: number };

const key = new PluginKey<DecorationSet>('imageUpload');

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
        options.onProgress(pending, false);
        const finish = (uploaded: string | undefined): void => {
          // An editor locked meanwhile (a conflict to settle) takes nothing in.
          const fileId = view.isDestroyed || view.editable ? uploaded : undefined;
          pending -= 1;
          options.onProgress(pending, fileId === undefined);
          if (view.isDestroyed) return;
          // Where the placeholder is now; it has moved with whatever was typed meanwhile.
          const found = key
            .getState(view.state)
            ?.find(undefined, undefined, (spec: { id?: number }) => spec.id === id);
          const at = found?.[0]?.from;
          const transaction = view.state.tr.setMeta(key, { remove: id } satisfies Change);
          const image = view.state.schema.nodes.image;
          if (fileId !== undefined && fileId !== '' && at !== undefined && image)
            transaction.replaceRangeWith(at, at, image.create({ fileId }));
          view.dispatch(transaction);
        };
        upload(file).then(finish, () => {
          finish(undefined);
        });
      }
      return true;
    }

    return [
      new Plugin<DecorationSet>({
        key,
        state: {
          init: () => DecorationSet.empty,
          apply(transaction, set) {
            let next = set.map(transaction.mapping, transaction.doc);
            const change = transaction.getMeta(key) as Change | undefined;
            if (change && 'add' in change) {
              const { id, position, label } = change.add;
              const widget = Decoration.widget(
                Math.min(position, transaction.doc.content.size),
                (view) => {
                  const element = view.dom.ownerDocument.createElement('span');
                  element.setAttribute('class', 'doc-upload');
                  element.setAttribute('role', 'img');
                  element.setAttribute('aria-label', label);
                  return element;
                },
                { id, key: `doc-upload-${id}` },
              );
              next = next.add(transaction.doc, [widget]);
            } else if (change && 'remove' in change) {
              next = next.remove(
                next.find(
                  undefined,
                  undefined,
                  (spec: { id?: number }) => spec.id === change.remove,
                ),
              );
            }
            return next;
          },
        },
        props: {
          decorations: (state) => key.getState(state),
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
