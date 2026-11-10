/**
 * Where a dragged row lands, in the terms the board store asks for.
 *
 * The table knows the place among the rows it shows. The store
 * (`BoardStore.dropCards`) counts among every card of the board that has the
 * group's value, in the manual order, whether the view's filter or the
 * search shows it or not. With a filter the two differ, so the place is
 * translated here: the row goes right after the shown row above it.
 */
import { manualComparator } from '$lib/logic/sort.ts';
import type { Card } from '$lib/logic/wire.ts';

/**
 * The cards the store counts a place among: those of the board in the manual
 * order, without the moved ones, and with `optionId` given only those whose
 * grouping property has that value (`''`: no value).
 */
export function manualColumn(
  cards: readonly Card[],
  cardOrder: readonly string[],
  groupById: string | undefined,
  optionId: string | undefined,
  moved: readonly string[],
): string[] {
  const valueOf = (card: Card): string => {
    const value = groupById ? card.fields.properties[groupById] : undefined;
    return typeof value === 'string' ? value : '';
  };
  return [...cards]
    .sort(manualComparator(cardOrder))
    .filter((card) => !moved.includes(card.id))
    .filter((card) => optionId === undefined || valueOf(card) === optionId)
    .map((card) => card.id);
}

/**
 * The place in `column` for a drop at `index` among the `shown` rows (both
 * without the moved ones): right after the nearest shown row above that the
 * column has, else right before the nearest one below, else at the end.
 */
export function storeIndex(
  column: readonly string[],
  shown: readonly string[],
  index: number,
): number {
  const place = new Map(column.map((id, at) => [id, at]));
  for (let above = Math.min(index, shown.length) - 1; above >= 0; above -= 1) {
    const at = place.get(shown[above] ?? '');
    if (at !== undefined) return at + 1;
  }
  for (let below = Math.max(index, 0); below < shown.length; below += 1) {
    const at = place.get(shown[below] ?? '');
    if (at !== undefined) return at;
  }
  return column.length;
}
