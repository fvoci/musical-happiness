/**
 * A sheet drawn in the frame (src/viewer-frame/xlsx-grid.ts) takes what the
 * workbook says as text: values, the literals of a format code and the
 * sheet's name that look like markup, or like markup escaped, are drawn as
 * they are and never read as markup.
 */
import { Blob as NodeBlob } from 'node:buffer';

import { afterEach, expect, it, vi } from 'vitest';

import { SheetView } from '../../src/viewer-frame/xlsx-grid.ts';
import { Looks } from '../../src/viewer-frame/xlsx-look.ts';
import { readWorkbook } from '../../src/viewer-frame/xlsx-read.ts';
import { repack } from '../../src/viewer-frame/zip-guard.ts';
import { cell, esc, workbook } from '../support/viewer-xlsx.ts';

const VALUES = ['<img src="x" onerror="globalThis.drawn = true">', '<b>굵게</b>', '&lt;i&gt;'];
const NAME = '<script>표</script>';
const CODE = '"<u>"0"</u>"';
const INLINE = '<svg onload="globalThis.drawn = true"></svg>';

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
  Reflect.deleteProperty(document.documentElement, 'clientWidth');
  Reflect.deleteProperty(document.documentElement, 'clientHeight');
});

it('draws values, format literals and the sheet name that look like markup as text', async () => {
  const cells = [
    cell('A1', 0, 's'),
    cell('B1', 1, 's'),
    cell('C1', 2, 's'),
    cell('D1', 5, undefined, 1),
    cell('E1', '', 'inlineStr', 0, `<is><t>${esc(INLINE)}</t></is>`),
  ].join('');
  const bytes = workbook({
    sheets: [{ name: NAME, xml: `<sheetData><row r="1">${cells}</row></sheetData>` }],
    strings: VALUES,
    stylesXml: `<numFmts count="1"><numFmt numFmtId="164" formatCode="${esc(CODE)}"/></numFmts><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164" applyNumberFormat="1"/></cellXfs>`,
  });
  // Read as the worker reads it; jsdom's Blob cannot stream, the worker's can.
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

  const drawn = [...view.element.querySelectorAll('[role="cell"]')].map((each) => each.textContent);
  expect(drawn).toEqual([...VALUES, '<u>5</u>', INLINE]);
  expect(view.element.querySelector('img, b, i, u, svg, script')).toBeNull();
  expect(view.element.querySelector('[role="table"]')?.getAttribute('aria-label')).toBe(NAME);
  expect(Reflect.get(globalThis, 'drawn')).toBeUndefined();
});
