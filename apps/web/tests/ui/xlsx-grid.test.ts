/**
 * A sheet drawn in the frame (src/viewer-frame/xlsx-grid.ts, xlsx-pane.ts,
 * xlsx-axis.ts): what the workbook says is taken as text (values, the
 * literals of a format code and the sheet's name that look like markup, or
 * like markup escaped, are drawn as they are and never read as markup); on
 * a sheet too dense to draw all of in view, the frozen headings are drawn
 * first and every cell looked at spends the budget, a merge's too; hidden
 * rows hidden one by one are one run to step over.
 */
import { Blob as NodeBlob } from 'node:buffer';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { Axis } from '../../src/viewer-frame/xlsx-axis.ts';
import { SheetView } from '../../src/viewer-frame/xlsx-grid.ts';
import { Looks } from '../../src/viewer-frame/xlsx-look.ts';
import { CELL_BUDGET } from '../../src/viewer-frame/xlsx-pane.ts';
import { readWorkbook } from '../../src/viewer-frame/xlsx-read.ts';
import { repack } from '../../src/viewer-frame/zip-guard.ts';
import { cell, esc, workbook } from '../support/viewer-xlsx.ts';

const VALUES = ['<img src="x" onerror="globalThis.drawn = true">', '<b>굵게</b>', '&lt;i&gt;'];
const NAME = '<script>표</script>';
const CODE = '"<u>"0"</u>"';
const INLINE = '<svg onload="globalThis.drawn = true"></svg>';
/** Columns and rows a pixel each: a 1000 by 600 view holds every cell of the sheets below. */
const TINY = `<sheetFormatPr defaultRowHeight="0.75"/><cols><col min="1" max="16384" width="0.1" customWidth="1"/></cols>`;

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
  Reflect.deleteProperty(document.documentElement, 'clientWidth');
  Reflect.deleteProperty(document.documentElement, 'clientHeight');
});

/** The first sheet of the workbook, read as the worker reads it and drawn in a view of 1000 by 600. */
async function drawn(bytes: Buffer): Promise<SheetView> {
  // jsdom's Blob cannot stream, the worker's can.
  vi.stubGlobal('Blob', NodeBlob);
  const book = await readWorkbook(await repack(new Uint8Array(bytes).buffer));
  vi.unstubAllGlobals();
  const [sheet] = book.sheets;
  if (!sheet) throw new Error('no sheet');
  // The frame's size: jsdom lays nothing out.
  Object.defineProperty(document.documentElement, 'clientWidth', {
    value: 1000,
    configurable: true,
  });
  Object.defineProperty(document.documentElement, 'clientHeight', {
    value: 600,
    configurable: true,
  });
  const view = new SheetView(sheet, new Looks(book, 'ko'));
  document.body.append(view.element);
  view.draw();
  return view;
}

/** Rows of `columns` cells holding numbers, from row `from`. */
function rowsOf(count: number, columns: number, from = 1): string {
  let xml = '';
  for (let row = from; row < from + count; row += 1) {
    let cells = '';
    for (let column = 0; column < columns; column += 1) {
      cells += `<c r="${letters(column)}${row}"><v>${row}</v></c>`;
    }
    xml += `<row r="${row}">${cells}</row>`;
  }
  return xml;
}

function letters(index: number): string {
  let name = '';
  for (let rest = index + 1; rest > 0; rest = Math.floor((rest - 1) / 26)) {
    name = String.fromCharCode(65 + ((rest - 1) % 26)) + name;
  }
  return name;
}

describe('what a sheet says', () => {
  it('draws values, format literals and the sheet name that look like markup as text', async () => {
    const cells = [
      cell('A1', 0, 's'),
      cell('B1', 1, 's'),
      cell('C1', 2, 's'),
      cell('D1', 5, undefined, 1),
      cell('E1', '', 'inlineStr', 0, `<is><t>${esc(INLINE)}</t></is>`),
    ].join('');
    const view = await drawn(
      workbook({
        sheets: [{ name: NAME, xml: `<sheetData><row r="1">${cells}</row></sheetData>` }],
        strings: VALUES,
        stylesXml: `<numFmts count="1"><numFmt numFmtId="164" formatCode="${esc(CODE)}"/></numFmts><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164" applyNumberFormat="1"/></cellXfs>`,
      }),
    );
    const texts = [...view.element.querySelectorAll('[role="cell"]')].map(
      (each) => each.textContent,
    );
    expect(texts).toEqual([...VALUES, '<u>5</u>', INLINE]);
    expect(view.element.querySelector('img, b, i, u, svg, script')).toBeNull();
    expect(view.element.querySelector('[role="table"]')?.getAttribute('aria-label')).toBe(NAME);
    expect(Reflect.get(globalThis, 'drawn')).toBeUndefined();
  });
});

describe('a sheet too dense to draw all of', () => {
  it('draws its frozen headings first, and the rest with what the budget leaves', async () => {
    // 101 rows of 100 cells in view, the first frozen: more cells than the budget.
    const pane = `<sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" state="frozen"/></sheetView></sheetViews>`;
    const view = await drawn(
      workbook({
        sheets: [
          { name: 'dense', xml: `${pane}${TINY}<sheetData>${rowsOf(101, 100)}</sheetData>` },
        ],
      }),
    );
    const heading = view.element.querySelectorAll('[role="row"][aria-rowindex="1"] [role="cell"]');
    expect(heading).toHaveLength(100);
    const all = view.element.querySelectorAll('[role="cell"]');
    expect(all.length).toBeLessThanOrEqual(CELL_BUDGET);
    expect(all.length).toBeGreaterThan(CELL_BUDGET - 100);
  });

  it('spends the budget on the cells under a merge too, which are looked at though not drawn', async () => {
    // 100 rows of 100 cells in view, the left half of them under one merge.
    const xml = `${TINY}<sheetData>${rowsOf(100, 100)}</sheetData><mergeCells count="1"><mergeCell ref="A1:AX100"/></mergeCells>`;
    const view = await drawn(workbook({ sheets: [{ name: 'merged', xml }] }));
    const rows = view.element.querySelectorAll('[role="row"]').length;
    // The merge, and the cells right of it.
    const plain = view.element.querySelectorAll('[role="cell"]').length - 1;
    // Each row walked looks at its 50 cells under the merge and draws up to 50 more.
    expect(plain + 50 * rows).toBeLessThanOrEqual(CELL_BUDGET + 50);
    expect(plain).toBeLessThan(100 * 50);
    expect(plain).toBeGreaterThan(0);
  });
});

describe('the rows along a sheet', () => {
  it('are one run where rows next to each other hide one by one, and step over it at once', () => {
    const hidden = Array.from(
      { length: 100_000 },
      (_, index) => [index + 1, index + 1, 0] as const,
    );
    const axis = new Axis(200_000, 20, hidden);
    expect(axis.next(0)).toBe(100_001);
    expect(axis.size(50_000)).toBe(0);
    expect(axis.start(100_001)).toBe(20);
    expect(axis.indexAt(20)).toBe(100_001);
    expect(axis.total).toBe(100_000 * 20);
  });
});
