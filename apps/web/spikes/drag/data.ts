/** A made-up board: nothing is loaded and nothing is saved. */
import type { ListOrder } from '$lib/logic/drop-target.ts';

export interface Card {
  id: string;
  title: string;
  tag: string | undefined;
}

export interface Column {
  id: string;
  title: string;
}

export interface BoardData {
  columns: Column[];
  cards: Record<string, Card>;
  order: ListOrder[];
}

/**
 * Cards per column. Six columns are wider than a window of 1280 pixels, and
 * all but the shortest are taller than one of 720.
 */
export const DEFAULT_SIZES = [12, 30, 6, 18, 9, 24];

const COLUMNS = ['Backlog', 'Ready', 'Doing', 'Review', 'Done', 'Archive'];

// Of different lengths on purpose: the cards come out one to four lines high.
const TASKS = [
  'Import from CSV',
  'Let a card be moved to another board without losing its comments',
  'Dark appearance',
  'Sort by more than one property at a time',
  'Remember the width of every table column for each person separately, not for the whole board',
  'Filter by date',
  'Show who else is looking at the card',
  'Print a card',
];
const TAGS = ['bug', 'design', undefined, 'docs', undefined, undefined];

/**
 * The sizes asked for in the address (`?sizes=3,0,500`), for trying an empty
 * column or a very long one; the default board otherwise.
 */
export function sizesFrom(search: string): number[] {
  const sizes = (new URLSearchParams(search).get('sizes') ?? '')
    .split(',')
    .filter((part) => /^\d+$/.test(part))
    .map(Number);
  return sizes.length > 0 ? sizes : DEFAULT_SIZES;
}

export function fakeBoard(sizes: readonly number[]): BoardData {
  const board: BoardData = { columns: [], cards: {}, order: [] };
  sizes.forEach((size, at) => {
    const title = COLUMNS[at] ?? `Column ${at + 1}`;
    const id = title.toLowerCase().replace(' ', '-');
    const items: string[] = [];
    for (let number = 1; number <= size; number += 1) {
      const cardId = `${id}-${number}`;
      const task = TASKS[(number + at * 3) % TASKS.length] ?? '';
      board.cards[cardId] = {
        id: cardId,
        title: `${title} ${number}: ${task}`,
        tag: TAGS[(number + at) % TAGS.length],
      };
      items.push(cardId);
    }
    board.columns.push({ id, title });
    board.order.push({ id, items });
  });
  return board;
}
