/**
 * The renderer of each kind of file, fetched the first time a file of that
 * kind is opened. Each is a chunk of its own with its library, so opening a
 * PDF fetches neither the Word nor the PowerPoint renderer.
 *
 * Adding a kind: its line in `FORMATS` of `$lib/file-viewer/formats.ts`, a module
 * here that exports `render`, and its line in this table.
 */
import type { FormatId } from '$lib/file-viewer/formats.ts';

import type { Render } from './renderer.ts';

export const RENDERERS: Record<FormatId, () => Promise<Render>> = {
  pdf: async () => (await import('./pdf.ts')).render,
  docx: async () => (await import('./docx.ts')).render,
  pptx: async () => (await import('./pptx.ts')).render,
};
