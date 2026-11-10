/**
 * Where dragged cards land in the manual order of a view.
 *
 * A view keeps one flat list of card identifiers (`cardOrder`), whatever its
 * columns are. Dropping cards at a place in a column means: take them out of
 * the list, and put them back just before the card that is at that place in
 * the column, or just after the column's last card when they were dropped at
 * its end.
 */

/**
 * The new `cardOrder`.
 *
 * - `shown`: every card of the view in the order it is shown now.
 * - `column`: the cards of the column the drop is in, in shown order,
 *   **without** the moved ones.
 * - `moved`: the cards that were dropped, in the order they are to keep.
 * - `index`: the place in `column` they were dropped at.
 */
export function droppedCardOrder(
  shown: readonly string[],
  column: readonly string[],
  moved: readonly string[],
  index: number,
): string[] {
  const carried = new Set(moved);
  const rest = shown.filter((id) => !carried.has(id));
  const before = column[Math.max(0, index)];
  const last = column.at(-1);
  let at: number;
  if (before !== undefined && index < column.length) at = rest.indexOf(before);
  else if (last !== undefined) at = rest.indexOf(last) + 1;
  else at = rest.length;
  if (at < 0) at = rest.length;
  return [...rest.slice(0, at), ...moved, ...rest.slice(at)];
}
