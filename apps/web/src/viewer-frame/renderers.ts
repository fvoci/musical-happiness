/**
 * The renderer of each kind of file, fetched the first time a file of that
 * kind is opened. Each is a chunk of its own with its library, so opening a
 * PDF does not fetch the Word renderer.
 *
 * Adding a kind: its line in `FORMATS` of `$lib/file-viewer/formats.ts`, a module
 * here that exports `render`, and its line in this table.
 */
import type { FormatId } from '$lib/file-viewer/formats.ts';

import type { Render } from './renderer.ts';

export const RENDERERS: Record<FormatId, () => Promise<Render>> = {
  pdf: async () => (await import('./pdf.ts')).render,
  docx: async () => (await import('./docx.ts')).render,
  hwpx: async () => (await import('./hwp.ts')).renderer('hwpx'),
  hwp: async () => (await import('./hwp.ts')).renderer('hwp'),
};

/**
 * The kinds whose renderer runs in a worker of its own and checks the file
 * there (an archive unpacked and counted, a compound file's chains): the
 * frame hands it the bytes as they came.
 */
export const CHECKED_IN_WORKER: ReadonlySet<FormatId> = new Set(['hwpx', 'hwp']);
