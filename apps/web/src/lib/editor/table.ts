/**
 * The table of a body: rows of cells and headers, a cell spanning columns
 * and rows. The document holds each cell's two spans and nothing else of its
 * look (`schema.ts`): no widths, no alignment.
 *
 * A table stands only in the document itself, not among the blocks of a
 * quote, a list entry or a cell, so that no table is in another: a cell
 * holds blocks, and so does a list entry in a cell. Its grid is filled when
 * it is stored (`table-grid.ts`); the server mends one that a merge of two
 * edits left unfilled. (The table plugin pads an unfilled table after any
 * change, another editor's too, so two editors that see one each pad it.)
 */
import { Document } from '@tiptap/extension-document';
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table';

import { TABLE_CELL_MIN_WIDTH } from './drawing.ts';
import { allowedSpan, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS } from './table-grid.ts';

/** The group of what stands only in the document itself. */
const DOCUMENT_ONLY = 'documentBlock';

/** The document: blocks, and tables. */
export const BodyDocument = Document.extend({ content: `(block | ${DOCUMENT_ONLY})+` });

/** Columns keep the width the page gives them: none is stored, and none is dragged. */
export const BodyTable = Table.extend({ group: DOCUMENT_ONLY }).configure({
  resizable: false,
  cellMinWidth: TABLE_CELL_MIN_WIDTH,
});

/** A span read from pasted HTML by its leading digits, as a browser reads it; else one. */
function spanOf(element: HTMLElement, name: string, limit: number): number {
  return allowedSpan(Number.parseInt(element.getAttribute(name) ?? '1', 10), limit) ?? 1;
}

/** The spans of a cell, the only attributes it keeps. */
function spans() {
  return {
    colspan: {
      default: 1,
      parseHTML: (element: HTMLElement) => spanOf(element, 'colspan', MAX_TABLE_COLUMNS),
    },
    rowspan: {
      default: 1,
      parseHTML: (element: HTMLElement) => spanOf(element, 'rowspan', MAX_TABLE_ROWS),
    },
  };
}

export const BodyTableCell = TableCell.extend({ addAttributes: spans });
export const BodyTableHeader = TableHeader.extend({ addAttributes: spans });
export { TableRow as BodyTableRow };
