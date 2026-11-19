/**
 * What a Hangul document holds, as rhwp reads it: for the tests that a saved
 * document is the document (hwp-roundtrip.test.ts). Two documents are
 * compared by their pages, the text drawn on them, their tables and their
 * pictures, and by how many controls of each kind they have.
 */
import { createHash } from 'node:crypto';

import type { HwpDocument } from '@rhwp/core';

export interface Content {
  pages: number;
  /** The text drawn on all pages, in order, its white space left out. */
  text: string;
  /** Each table once, in the document's order: its rows and columns. */
  tables: string[];
  /** Each picture once: its size as drawn, and what its bytes hash to. */
  pictures: string[];
  /** How many controls of each kind there are (sections, headers, tables, fields, …). */
  controls: Record<string, number>;
}

interface Placed {
  type?: string;
  w?: number;
  h?: number;
  rowCount?: number;
  colCount?: number;
  secIdx?: number;
  paraIdx?: number;
  controlIdx?: number;
  stableIndex?: number[];
}

function pictureOf(document: HwpDocument, placed: Placed): string {
  const size = `${String(placed.w)}x${String(placed.h)}`;
  try {
    const { secIdx = 0, paraIdx = 0, controlIdx = 0 } = placed;
    const data = document.getControlImageData(secIdx, paraIdx, '[]', controlIdx);
    return `${size}:${createHash('sha256').update(data).digest('hex').slice(0, 16)}`;
  } catch {
    // One rhwp does not hand out by its place (inside a cell): its size alone.
    return `${size}:not-read`;
  }
}

export function contentOf(document: HwpDocument): Content {
  const pages = document.pageCount();
  const text: string[] = [];
  const tables = new Map<string, string>();
  const pictures = new Map<string, string>();
  for (let page = 0; page < pages; page += 1) {
    const layout = JSON.parse(document.getPageTextLayout(page)) as { runs?: { text?: string }[] };
    text.push((layout.runs ?? []).map((run) => run.text ?? '').join(''));
    const placed = JSON.parse(document.getPageControlLayout(page)) as { controls?: Placed[] };
    for (const control of placed.controls ?? []) {
      const key = JSON.stringify(
        control.stableIndex ?? [control.secIdx, control.paraIdx, control.controlIdx],
      );
      if (control.type === 'table' && !tables.has(key)) {
        tables.set(key, `${String(control.rowCount)}x${String(control.colCount)}`);
      }
      if (control.type === 'image' && !pictures.has(key)) {
        pictures.set(key, pictureOf(document, control));
      }
    }
  }
  const controls: Record<string, number> = {};
  for (const control of JSON.parse(document.getControls()) as { ctrlId?: string }[]) {
    const id = control.ctrlId ?? '?';
    controls[id] = (controls[id] ?? 0) + 1;
  }
  return {
    pages,
    text: text.join('').replace(/\s+/g, ''),
    tables: [...tables.values()],
    pictures: [...pictures.values()],
    controls,
  };
}

/** What of `before` is not in `after` as it was: the names of what differs, none when nothing does. */
export function differences(before: Content, after: Content): string[] {
  const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);
  const sorted = (controls: Record<string, number>): [string, number][] =>
    Object.entries(controls).sort(([a], [b]) => a.localeCompare(b));
  return [
    ...(before.pages === after.pages ? [] : ['pages']),
    ...(before.text === after.text ? [] : ['text']),
    ...(same(before.tables, after.tables) ? [] : ['tables']),
    ...(same(before.pictures, after.pictures) ? [] : ['pictures']),
    ...(same(sorted(before.controls), sorted(after.controls)) ? [] : ['controls']),
  ];
}
