/**
 * The style a cell not written in a sheet takes from its row or its
 * column: a whole row or column filled or bordered shows on its empty
 * cells as well. A cell written in the sheet shows its own style, whatever
 * its row and column have (its `s`, 0 when it names none); one not written
 * takes its row's (`row@s` with `customFormat`), else its column's
 * (`col@style`), as Excel draws them: Excel writes the cells where a styled
 * row crosses a styled column with a style of their own, so the row's
 * standing over the column's matters only for files other programs wrote.
 *
 * Only the cells in view are looked at (`xlsx-pane.ts`), and only those
 * whose style paints something (a fill or a border): a style of font alone
 * shows nothing on an empty cell.
 */
import type { Sheet } from './xlsx-protocol.ts';

/** A run of columns of one style. */
interface Run {
  first: number;
  last: number;
  style: number;
}

export class BlankStyles {
  readonly #rows: Uint32Array;
  readonly #runs: Run[] = [];
  readonly #paints: (style: number) => boolean;
  /** Some column has a style that paints: the columns of a row without one are looked at. */
  readonly columnsPaint: boolean;

  /** `paints` says whether a style paints anything on an empty cell. */
  constructor(sheet: Sheet, paints: (style: number) => boolean) {
    this.#rows = sheet.rowStyles;
    this.#paints = paints;
    const runs: Run[] = [];
    const given = sheet.columnStyles;
    for (let at = 0; at + 2 < given.length; at += 3) {
      runs.push({ first: given[at] ?? 0, last: given[at + 1] ?? 0, style: given[at + 2] ?? 0 });
    }
    // Where runs overlap, the one that starts first wins, as the columns' widths do (`Axis`).
    runs.sort((a, b) => a.first - b.first);
    let end = -1;
    for (const run of runs) {
      const first = Math.max(run.first, end + 1);
      if (first > run.last) continue;
      this.#runs.push({ first, last: run.last, style: run.style });
      end = run.last;
    }
    this.columnsPaint = this.#runs.some((run) => paints(run.style));
  }

  /** Whether a style paints anything on an empty cell. */
  paints(style: number): boolean {
    return style >= 0 && this.#paints(style);
  }

  /** The style row `row` gives its cells not written; -1 when it gives none. */
  row(row: number): number {
    const rows = this.#rows;
    let low = 0;
    let high = rows.length / 2 - 1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      const at = rows[middle * 2] ?? 0;
      if (at === row) return rows[middle * 2 + 1] ?? 0;
      if (at < row) low = middle + 1;
      else high = middle - 1;
    }
    return -1;
  }

  /** The style column `column` gives its cells not written; -1 when it gives none. */
  column(column: number): number {
    let low = 0;
    let high = this.#runs.length - 1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      const run = this.#runs[middle];
      if (!run) break;
      if (column < run.first) high = middle - 1;
      else if (column > run.last) low = middle + 1;
      else return run.style;
    }
    return -1;
  }

  /** The style a cell not written takes: its row's, else its column's; -1 for none. */
  at(row: number, column: number): number {
    const own = this.row(row);
    return own >= 0 ? own : this.column(column);
  }
}
