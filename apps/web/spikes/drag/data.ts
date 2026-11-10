/** A made-up board: nothing is loaded and nothing is saved. */
import type { ListOrder } from '$lib/logic/drop-target.ts';

export interface Card {
  id: string;
  title: string;
  /** What the sorted view sorts by. */
  task: string;
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

/** What the page starts with, from its address. */
export interface Start {
  /** Cards per column: `?sizes=3,0,500`, for trying an empty column or a very long one. */
  sizes: number[];
  /** `?sorted=1`: the sorted view is on. */
  sorted: boolean;
  /** `?select=1`: a tap selects a card instead of opening it. */
  selecting: boolean;
  /** `?zone=1`: the hidden column is shown as a place to drop cards on. */
  zone: boolean;
  /** `?dir=rtl`: the page runs right to left. */
  rtl: boolean;
}

export function startFrom(search: string): Start {
  const params = new URLSearchParams(search);
  const sizes = (params.get('sizes') ?? '')
    .split(',')
    .filter((part) => /^\d+$/.test(part))
    .map(Number);
  return {
    sizes: sizes.length > 0 ? sizes : DEFAULT_SIZES,
    sorted: params.get('sorted') === '1',
    selecting: params.get('select') === '1',
    zone: params.get('zone') === '1',
    rtl: params.get('dir') === 'rtl',
  };
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
        task,
        tag: TAGS[(number + at) % TAGS.length],
      };
      items.push(cardId);
    }
    board.columns.push({ id, title });
    board.order.push({ id, items });
  });
  return board;
}

/**
 * The order of the sorted view: by task, and among cards with the same task
 * by id, numbers counted as numbers. Returns a comparison of two card ids.
 */
export function byTask(cards: Record<string, Card>): (first: string, second: string) => number {
  const collator = new Intl.Collator('en', { numeric: true });
  return (first, second) =>
    collator.compare(cards[first]?.task ?? '', cards[second]?.task ?? '') ||
    collator.compare(first, second);
}
