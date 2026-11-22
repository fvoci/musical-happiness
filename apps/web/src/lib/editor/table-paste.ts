/**
 * Tables pasted into a body.
 *
 * - Text that holds a Markdown table (from a Markdown file, a chat, a
 *   terminal) is read as Markdown (markdown.ts), so that its table is a
 *   table; other text is pasted as text, and so is any pasted as plain text
 *   (Ctrl+Shift+V) or into a code block.
 * - A table past 200 rows or 20 columns, from text or from a page, is pasted
 *   as its rows, each a line of its cells' text between ` | `, as the
 *   Markdown reader writes one out.
 * - Into a table, a table pasted alone fills the cells from the one the
 *   reader is in (the table plugin's way, which no limit lets grow too far);
 *   a table pasted with more around it is pasted as its rows, since no
 *   table stands in a cell.
 */
import { Fragment, Slice, type Node as DocumentNode, type Schema } from '@tiptap/pm/model';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { isInTable, TableMap } from '@tiptap/pm/tables';

import { markdownToBlocks } from './markdown.ts';
import { MAX_TABLE_COLUMNS, MAX_TABLE_ROWS } from './table-grid.ts';

/** Whether a table is past 200 rows or 20 columns. */
export function isPastLimits(table: DocumentNode): boolean {
  const map = TableMap.get(table);
  return map.height > MAX_TABLE_ROWS || map.width > MAX_TABLE_COLUMNS;
}

/** What a parser of the clipboard gives back for what it leaves to the editor's own. */
const LEFT = undefined as unknown as Slice;

/** A line of a Markdown table that parts its head from its body: `|---|:--:|`. */
const DELIMITER_ROW = /^ {0,3}\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/m;

/** The blocks of a Markdown text that holds a table; undefined for any other text. */
export function markdownTable(text: string, schema: Schema): DocumentNode[] | undefined {
  if (!text.includes('|') || !DELIMITER_ROW.test(text)) return undefined;
  const { content, notes } = markdownToBlocks(text);
  // A table past the limits is in it as its rows.
  if (!content.some((node) => node.type === 'table') && !notes.table) return undefined;
  return content.map((node) => schema.nodeFromJSON(node));
}

/** A table's rows as lines: each the text of its cells between ` | `, each cell's blocks between spaces. */
export function linesOf(table: DocumentNode, schema: Schema): DocumentNode[] {
  const lines: DocumentNode[] = [];
  const paragraph = schema.nodes.paragraph;
  if (!paragraph) return lines;
  table.forEach((row) => {
    const parts: DocumentNode[] = [];
    row.forEach((cell, _offset, index) => {
      if (index > 0) parts.push(schema.text(' | '));
      let first = true;
      cell.descendants((node) => {
        if (!node.isTextblock) return true;
        if (!first && node.childCount > 0) parts.push(schema.text(' '));
        if (node.childCount > 0) first = false;
        node.forEach((inline) => parts.push(inline));
        return false;
      });
    });
    if (parts.length > 0) lines.push(paragraph.create(null, parts));
  });
  return lines;
}

/** The pasted blocks with each table that may not stand as one given as its rows. */
function placed(content: Fragment, schema: Schema, inTable: boolean): Fragment {
  const alone = content.childCount === 1;
  const nodes: DocumentNode[] = [];
  content.forEach((node) => {
    const isTable = node.type.spec.tableRole === 'table';
    if (isTable && (isPastLimits(node) || (inTable && !alone))) {
      nodes.push(...linesOf(node, schema));
    } else nodes.push(node);
  });
  return Fragment.fromArray(nodes);
}

export function tablePaste(): Plugin {
  return new Plugin({
    key: new PluginKey('tablePaste'),
    props: {
      clipboardTextParser(text, $context, plain, view) {
        if (plain || $context.parent.type.spec.code) return LEFT;
        const blocks = markdownTable(text, view.state.schema);
        if (!blocks) return LEFT;
        // A paragraph at either end joins the line pasted into; a table stays whole.
        return Slice.maxOpen(Fragment.fromArray(blocks), false);
      },
      transformPasted(slice, view) {
        const { schema } = view.state;
        const inTable = isInTable(view.state);
        if (!slice.content.content.some((node) => node.type.spec.tableRole === 'table'))
          return slice;
        const content = placed(slice.content, schema, inTable);
        if (content.eq(slice.content)) return slice;
        // Open no further than it was, nor than what it now holds can be.
        const open = Slice.maxOpen(content, false);
        return new Slice(
          content,
          Math.min(slice.openStart, open.openStart),
          Math.min(slice.openEnd, open.openEnd),
        );
      },
    },
  });
}
