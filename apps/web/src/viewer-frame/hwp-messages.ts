/**
 * What the HWP renderer (hwp.ts) and its worker (hwp-worker.ts) say to each
 * other. The worker answers each message with one: `open` with `opened` or
 * `failed`, `page` with `page` or `failed`.
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
  /** The file, and the renderer's WebAssembly as hexadecimal text (virtual:rhwp-wasm). */
  | { kind: 'open'; format: HwpFormat; bytes: ArrayBuffer; wasm: string }
  /** A page, from 1. */
  | { kind: 'page'; page: number };

export type FromWorker =
  | { kind: 'opened'; pages: PageSize[] }
  | { kind: 'page'; svg: string; text: TextRun[] }
  | { kind: 'failed'; reason: 'locked' | 'unreadable' | 'too-large' | 'renderer' };
