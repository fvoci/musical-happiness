/**
 * Where the cells of a table fall: its rows one under another, each cell in
 * the first column of its row that no cell above still covers, as the editor
 * lays a table out (`prosemirror-tables`). The server's rule is the same
 * (`crates/model/src/table_grid.rs`), and the shared vectors hold the two
 * together.
 *
 * A stored table fills its grid: no cell spans past the last row or over
 * another cell, and every row is as wide as the widest.
 */

/** The most rows a table has. */
export const MAX_TABLE_ROWS = 200;
/** The most columns a table has, as its cells' spans add up across a row. */
export const MAX_TABLE_COLUMNS = 20;

interface GridNode {
  attrs?: Record<string, unknown>;
  content?: GridNode[];
}

/** What is wrong with the grid of a table, if anything. */
export type GridProblem = 'tooLarge' | 'noCell' | 'unfilled';

/** A span a cell states: a whole number up to the limit, one if it has none, undefined if it is not allowed. */
function statedSpan(cell: GridNode, name: string, limit: number): number | undefined {
  const value = cell.attrs?.[name];
  if (value === undefined) return 1;
  return typeof value === 'number' && Number.isInteger(value) && value >= 1 && value <= limit
    ? value
    : undefined;
}

/**
 * What is wrong with the grid of a table: past a limit, no cell at all, or a
 * span that does not fit as stated or a row narrower than the table; nothing
 * when it fills its grid. What a child is, is not asked.
 */
export function gridProblem(table: GridNode): GridProblem | undefined {
  const rows = Array.isArray(table.content) ? table.content : [];
  if (rows.length > MAX_TABLE_ROWS) return 'tooLarge';
  const covered: boolean[][] = rows.map(() => []);
  const isCovered = (row: number, column: number): boolean => covered[row]?.[column] === true;
  let unfilled = false;
  for (const [row, node] of rows.entries()) {
    let column = 0;
    for (const cell of Array.isArray(node.content) ? node.content : []) {
      while (isCovered(row, column)) column += 1;
      const statedColumns = statedSpan(cell, 'colspan', MAX_TABLE_COLUMNS);
      const statedRows = statedSpan(cell, 'rowspan', MAX_TABLE_ROWS);
      let columns = 0;
      while (columns < (statedColumns ?? 1) && !isCovered(row, column + columns)) columns += 1;
      // No cell above covers these columns in this row, so none covers them below.
      const down = Math.min(statedRows ?? 1, rows.length - row);
      if (column + columns > MAX_TABLE_COLUMNS) return 'tooLarge';
      for (const line of covered.slice(row, row + down)) {
        for (let at = column; at < column + columns; at += 1) line[at] = true;
      }
      if (columns !== statedColumns || down !== statedRows) unfilled = true;
      column += columns;
    }
  }
  const width = Math.max(0, ...covered.map((line) => line.length));
  if (width === 0) return 'noCell';
  // Counted, not walked: a row's list has holes where nothing covers it.
  const filled = covered.every((line) => line.filter((at) => at).length === width);
  return unfilled || !filled ? 'unfilled' : undefined;
}
