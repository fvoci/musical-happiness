/**
 * The file open in the viewer: opened from a card's attachments, or from a
 * body's file block, in the editor or where a body is only shown, on any
 * screen of the app or of a public link. One at a time:
 * `FileViewerHost.svelte`, which the app's frame mounts once, shows it.
 *
 * When the viewer closes, the focus goes back where the file was opened
 * from; in the editor, to the editor, whose selection is still the block,
 * so that Delete deletes it and the arrows move on from it.
 */
import type { FileSource, FileViewerLink } from '$lib/editor/files.ts';
import { opensInViewer } from '$lib/file-viewer/extensions.ts';
import type { ViewedFile } from '$lib/file-viewer/session.svelte.ts';

import { viewerChunk } from './chunk.svelte.ts';

class Viewing {
  /** The file open, if any. */
  file = $state.raw<ViewedFile | undefined>(undefined);
  #from: HTMLElement | undefined;

  /**
   * Opens the viewer on a file once it has arrived; says whether it came
   * (the connection may have dropped). A second press while one is open,
   * or on its way, opens no other.
   */
  async open(file: ViewedFile, from: HTMLElement): Promise<boolean> {
    const arrived = await viewerChunk.load();
    if (arrived && !this.file) {
      this.#from = from;
      this.file = file;
    }
    return arrived;
  }

  close(): void {
    const from = this.#from;
    this.file = undefined;
    this.#from = undefined;
    if (!from?.isConnected) return;
    const editor = from.closest<HTMLElement>('[contenteditable="true"]');
    (editor ?? from).focus({ preventScroll: true });
  }
}

export const viewing = new Viewing();

/** How the file blocks of `source`'s bodies open the viewer; none for a source that cannot read bytes. */
export function viewerLinkOf(source: Pick<FileSource, 'bytes'>): FileViewerLink | undefined {
  const bytes = source.bytes;
  if (!bytes) return undefined;
  return {
    opens: (details) => opensInViewer(details.name, details.size),
    open: (fileId, details, from) =>
      viewing.open(
        {
          name: details.name,
          size: details.size,
          // Bytes that cannot be read are the viewer's to tell; the source has reported why.
          read: (signal) => bytes(fileId, signal).catch(() => undefined),
        },
        from,
      ),
  };
}
