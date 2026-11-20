/**
 * What the HWP renderer (hwp.ts) and its worker (hwp-worker.ts) say to each
 * other. The worker answers each message with one: `open` with `opened` or
 * `failed`, `page` with `page` or `failed`.
 *
 * The editor's frame (src/editor-frame/main.ts) asks the same worker for the
 * check alone, before the editor reads a file: `check`, answered with
 * `checked` or `failed`.
 */

export type HwpFormat = 'hwp' | 'hwpx';

/** A page's size in CSS pixels at the document's own size. */
export interface PageSize {
  width: number;
  height: number;
}

/** A run of a page's text where the page draws it: what a screen reader reads, and what is selected. */
export interface TextRun {
  text: string;
  /** Its box on the page, in CSS pixels. */
  x: number;
  y: number;
  width: number;
  height: number;
  /** Its font's size, in CSS pixels. */
  size: number;
}

export type ToWorker =
  /** The file, and rhwp's WebAssembly as the app fetched it ($lib/file-viewer/engine.ts). */
  | { kind: 'open'; format: HwpFormat; bytes: ArrayBuffer; wasm: ArrayBuffer }
  /** A page, from 1. */
  | { kind: 'page'; page: number }
  /** The file, to be checked as `open` checks it and handed back as rhwp may read it. */
  | { kind: 'check'; format: HwpFormat; bytes: ArrayBuffer };

export type FromWorker =
  | { kind: 'opened'; pages: PageSize[] }
  | { kind: 'page'; svg: string; text: TextRun[] }
  | { kind: 'checked'; bytes: ArrayBuffer }
  | { kind: 'failed'; reason: 'locked' | 'unreadable' | 'too-large' | 'renderer' };
