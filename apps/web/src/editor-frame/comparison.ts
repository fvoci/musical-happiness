/**
 * What a save is compared with: the document as the editor was given it.
 *
 * The frame keeps, as it opens a document, what it needs to tell what a save
 * of it leaves out (an HWPX's parts, parts.ts; an HWP's streams, streams.ts),
 * and compares each export with that before the app is handed the bytes.
 */
import type { EditFormat, Loss } from '$lib/file-viewer/editor-protocol.ts';
import type { ZipEntry } from '$lib/file-viewer/zip-directory.ts';

import { packageParts, partsLeftOut } from './parts.ts';
import { hwpStreams, streamsLeftOut, type HwpStreams } from './streams.ts';

export type Opened =
  { format: 'hwpx'; parts: readonly ZipEntry[] } | { format: 'hwp'; streams: HwpStreams };

/** What is kept of the bytes the editor was given. Throws on bytes it cannot read. */
export function openedOf(format: EditFormat, bytes: Uint8Array): Opened {
  return format === 'hwpx'
    ? { format, parts: packageParts(bytes) }
    : { format, streams: hwpStreams(bytes) };
}

/** What the saved bytes leave out of the document as it was opened. Throws on bytes it cannot read. */
export function leftOut(
  opened: Opened,
  saved: Uint8Array,
): { unprotected: boolean; losses: Loss[] } {
  return opened.format === 'hwpx'
    ? { unprotected: false, losses: partsLeftOut(opened.parts, saved) }
    : streamsLeftOut(opened.streams, saved);
}
