/**
 * Where a dragged row lands: from the place among the rows the table shows
 * to the place the board store counts, among every card of the group.
 */
import { describe, expect, it } from 'vitest';

import { manualColumn, storeIndex } from '$lib/features/views/table/drop-index.ts';
import { droppedCardOrder } from '$lib/state/card-order.ts';
import { createCard, type Card } from '$lib/logic/wire.ts';

function card(id: string, createAt: number, status?: string): Card {
  const made = createCard();
  made.id = id;
  made.createAt = createAt;
  if (status !== undefined) made.fields.properties.status = status;
  return made;
}

describe('the place in the column of the store', () => {
  it('is the same place when every card of the column is shown', () => {
    const column = ['a', 'b', 'c'];
    expect(storeIndex(column, column, 0)).toBe(0);
    expect(storeIndex(column, column, 2)).toBe(2);
    expect(storeIndex(column, column, 3)).toBe(3);
  });

  it('is right after the shown row above, past the cards a filter hides before it', () => {
    // `b` is hidden: the rows shown are a and c.
    expect(storeIndex(['a', 'b', 'c'], ['a', 'c'], 1)).toBe(1);
    // After c, which is the third of the column.
    expect(storeIndex(['a', 'b', 'c'], ['a', 'c'], 2)).toBe(3);
    // `a` is hidden: after b, the second.
    expect(storeIndex(['a', 'b', 'c'], ['b', 'c'], 1)).toBe(2);
  });

  it('is right before the first shown row when the drop is at the top', () => {
    // `a` is hidden and stays first.
    expect(storeIndex(['a', 'b', 'c'], ['b', 'c'], 0)).toBe(1);
  });

  it('skips a shown row the column does not have', () => {
    // `x` is shown in the group (its value is one no option has) but the store does not count it there.
    expect(storeIndex(['a', 'b'], ['a', 'x', 'b'], 2)).toBe(1);
    expect(storeIndex(['a', 'b'], ['x', 'a', 'b'], 1)).toBe(0);
  });

  it('is the end of the column when no shown row is in it: a folded or filtered-out group', () => {
    expect(storeIndex(['a', 'b'], [], 0)).toBe(2);
    expect(storeIndex([], [], 0)).toBe(0);
  });

  it('takes a place beyond the rows for the end', () => {
    expect(storeIndex(['a', 'b'], ['a', 'b'], 99)).toBe(2);
  });
});

describe('the column the store counts in', () => {
  const cards = [card('a', 1, 'todo'), card('b', 2, 'done'), card('c', 3, 'todo'), card('d', 4)];

  it('is every card in the manual order, without the moved ones', () => {
    expect(manualColumn(cards, ['c', 'a'], undefined, undefined, ['a'])).toEqual(['c', 'b', 'd']);
  });

  it('has only the cards of the group when one is named', () => {
    expect(manualColumn(cards, ['c', 'a', 'b'], 'status', 'todo', [])).toEqual(['c', 'a']);
    expect(manualColumn(cards, [], 'status', 'done', [])).toEqual(['b']);
  });

  it('has the cards without a value for the empty option', () => {
    expect(manualColumn(cards, [], 'status', '', [])).toEqual(['d']);
  });
});

describe('with the order the store then writes', () => {
  // The store's own arithmetic, given the place this file worked out: a row
  // dropped under the last shown row ends up under it, whatever is hidden.
  it('a row dropped after the last shown one lands right after it', () => {
    const cards = [card('a', 1), card('hidden', 2), card('c', 3)];
    const order = ['a', 'hidden', 'c'];
    const moved = ['a'];
    const column = manualColumn(cards, order, undefined, undefined, moved);
    const index = storeIndex(column, ['c'], 1);
    expect(droppedCardOrder(order, column, moved, index)).toEqual(['hidden', 'c', 'a']);
  });

  it('a row dropped between two shown ones lands right after the upper one', () => {
    const cards = [card('a', 1), card('b', 2), card('hidden', 3), card('d', 4), card('e', 5)];
    const order = ['a', 'b', 'hidden', 'd', 'e'];
    const moved = ['e'];
    const column = manualColumn(cards, order, undefined, undefined, moved);
    // Shown: a, b, d. Dropped between b and d.
    const index = storeIndex(column, ['a', 'b', 'd'], 2);
    expect(droppedCardOrder(order, column, moved, index)).toEqual(['a', 'b', 'e', 'hidden', 'd']);
  });
});
