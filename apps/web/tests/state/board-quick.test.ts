/**
 * The quick filters a reader has turned on: kept per board in the browser,
 * or for the visit; and the cards a view shows with them, between its filter
 * and its search, a card the reader made on the visit let through.
 */
import { afterEach, describe, expect, it } from 'vitest';

import { createCard, type BoardView, type Card, type Container } from '$lib/logic/wire.ts';
import {
  madeOnVisit,
  QUICK_FILTERS_KEY,
  QuickFilterChoices,
  quickFilters,
  type VisitedBoard,
} from '$lib/state/board-quick.svelte.ts';
import type { CardSummary } from '$lib/state/backend.ts';
import {
  cardsOfBoardView,
  filteredCardsOfBoardView,
  type BoardCardsSource,
} from '$lib/state/board-parts.ts';
import { forgetUnscoped, forgetUser } from '../../src/app/last-workspace.ts';

/** A storage of its own, as `localStorage` is. */
function memoryStorage(): Storage {
  const items = new Map<string, string>();
  return {
    get length() {
      return items.size;
    },
    clear: () => {
      items.clear();
    },
    getItem: (key) => items.get(key) ?? null,
    key: (index) => [...items.keys()][index] ?? null,
    removeItem: (key) => {
      items.delete(key);
    },
    setItem: (key, value) => {
      items.set(key, value);
    },
  };
}

describe('the chips of a board', () => {
  it('are kept in the browser under the board, as their names and nothing else', () => {
    const storage = memoryStorage();
    const choices = new QuickFilterChoices(() => storage);
    choices.toggle('board-1', 'overdue', 'browser');
    choices.toggle('board-1', 'mine', 'browser');
    expect(choices.chosen('board-1')).toEqual(['overdue', 'mine']);
    expect(storage.getItem(`${QUICK_FILTERS_KEY}board-1`)).toBe('["overdue","mine"]');
    expect(choices.chosen('board-2')).toEqual([]);
    // Another page reads them back.
    expect(new QuickFilterChoices(() => storage).chosen('board-1')).toEqual(['overdue', 'mine']);
  });

  it('are taken off one at a time, or all at once, and leave nothing behind', () => {
    const storage = memoryStorage();
    const choices = new QuickFilterChoices(() => storage);
    choices.toggle('board-1', 'overdue', 'browser');
    choices.toggle('board-1', 'incomplete', 'browser');
    choices.toggle('board-1', 'overdue', 'browser');
    expect(choices.chosen('board-1')).toEqual(['incomplete']);
    choices.clear('board-1', 'browser');
    expect(choices.chosen('board-1')).toEqual([]);
    expect(storage.length).toBe(0);
  });

  it('are kept for the visit alone for a reader without an account, or without storage', () => {
    const storage = memoryStorage();
    const choices = new QuickFilterChoices(() => storage);
    choices.toggle('board-1', 'overdue', 'visit');
    expect(choices.chosen('board-1')).toEqual(['overdue']);
    expect(storage.length).toBe(0);
    const blocked = new QuickFilterChoices(() => {
      throw new Error('storage is blocked');
    });
    blocked.toggle('board-1', 'overdue', 'browser');
    expect(blocked.chosen('board-1')).toEqual(['overdue']);
  });

  it('read nothing they do not know from storage', () => {
    const storage = memoryStorage();
    storage.setItem(`${QUICK_FILTERS_KEY}a`, '["mine","mine","everything",3]');
    storage.setItem(`${QUICK_FILTERS_KEY}b`, 'not json');
    storage.setItem(`${QUICK_FILTERS_KEY}c`, '{"mine":true}');
    const choices = new QuickFilterChoices(() => storage);
    expect(choices.chosen('a')).toEqual(['mine']);
    expect(choices.chosen('b')).toEqual([]);
    expect(choices.chosen('c')).toEqual([]);
  });

  it('are forgotten when the reader signs out, and when their account goes', () => {
    localStorage.setItem(`${QUICK_FILTERS_KEY}a`, '["mine"]');
    localStorage.setItem('fvoci:locale', 'ko');
    forgetUnscoped();
    expect(localStorage.getItem(`${QUICK_FILTERS_KEY}a`)).toBeNull();
    expect(localStorage.getItem('fvoci:locale')).toBe('ko');
    localStorage.setItem(`${QUICK_FILTERS_KEY}b`, '["overdue"]');
    forgetUser('user-1');
    expect(localStorage.getItem(`${QUICK_FILTERS_KEY}b`)).toBeNull();
    localStorage.clear();
  });
});

describe('the cards made on a visit', () => {
  const card = (id: string, createdBy = 'me'): Card => ({ ...createCard(), id, createdBy });

  it('are those that were not there when the visit started and the reader made', () => {
    const board: { status: string; visit: number; realCards: Card[] } = {
      status: 'ready',
      visit: 1,
      realCards: [card('old')],
    };
    expect(madeOnVisit(board, 'me')(card('old'))).toBe(false);
    board.realCards = [card('old'), card('new'), card('theirs', 'you')];
    const made = madeOnVisit(board, 'me');
    expect(made(card('new'))).toBe(true);
    expect(made(card('theirs', 'you'))).toBe(false);
    expect(madeOnVisit(board, undefined)(card('new'))).toBe(false);
  });

  it('start being counted once the board is loaded, and anew on another visit', () => {
    const board: { status: string; visit: number; realCards: Card[] } = {
      status: 'loading',
      visit: 1,
      realCards: [],
    };
    expect(madeOnVisit(board, 'me')(card('old'))).toBe(false);
    board.status = 'ready';
    board.realCards = [card('old')];
    expect(madeOnVisit(board, 'me')(card('old'))).toBe(false);
    board.realCards = [card('old'), card('new')];
    expect(madeOnVisit(board, 'me')(card('new'))).toBe(true);
    board.visit = 2;
    expect(madeOnVisit(board, 'me')(card('new'))).toBe(false);
  });
});

describe('the cards a view shows', () => {
  const boardId = 'quick-board';
  afterEach(() => {
    quickFilters.clear(boardId, 'visit');
  });

  const definition = {
    id: boardId,
    cardProperties: [
      {
        id: 'status',
        name: 'Status',
        type: 'select',
        options: [
          { id: 'todo', label: 'To do', color: 'blue' },
          { id: 'finished', label: 'Done', color: 'green' },
        ],
      },
      { id: 'owner', name: 'Owner', type: 'person', options: [] },
      { id: 'due', name: 'Due', type: 'date', options: [] },
    ],
    doneWhen: { kind: 'option', propertyId: 'status', optionIds: ['finished'] },
    dueDatePropertyId: 'due',
  } as unknown as Container;

  const card = (
    id: string,
    title: string,
    properties: Card['fields']['properties'],
  ): CardSummary => {
    const made = createCard();
    return {
      ...made,
      id,
      title,
      createdBy: 'someone',
      fields: { ...made.fields, properties },
      hasBody: false,
      tasksTotal: 0,
      tasksDone: 0,
    };
  };

  const view = {
    id: 'view',
    fields: {
      viewType: 'table',
      sort: [],
      filter: { operation: 'and', filters: [] },
      cardOrder: [],
    },
  } as unknown as BoardView;

  function source(cards: CardSummary[], status = 'ready'): BoardCardsSource & VisitedBoard {
    return {
      boardId,
      status,
      visit: 1,
      realCards: cards,
      board: definition,
      cardsById: new Map(cards.map((each) => [each.id, each])),
      workspace: {
        userName: () => undefined,
        me: { id: 'me' },
        locale: 'en',
        regional: { timeZone: 'Asia/Seoul', weekStart: 1 },
      },
    };
  }

  const CARDS = [
    card('a', 'Plan the launch', { owner: 'me', status: 'todo', due: { from: '2020-01-01' } }),
    card('b', 'Plan the party', { owner: 'you', status: 'todo' }),
    card('c', 'Write the plan', { owner: 'me', status: 'finished' }),
  ];
  const titles = (cards: readonly Card[]): string[] => cards.map((each) => each.title);

  it('are narrowed by the chips after the filter of the view, and by the search after the chips', () => {
    const shown = source(CARDS);
    expect(titles(filteredCardsOfBoardView(shown, view))).toHaveLength(3);
    quickFilters.toggle(boardId, 'mine', 'visit');
    expect(titles(filteredCardsOfBoardView(shown, view))).toEqual([
      'Plan the launch',
      'Write the plan',
    ]);
    quickFilters.toggle(boardId, 'incomplete', 'visit');
    expect(titles(cardsOfBoardView(shown, view))).toEqual(['Plan the launch']);
    expect(titles(cardsOfBoardView(shown, view, 'party'))).toEqual([]);
    quickFilters.clear(boardId, 'visit');
    expect(titles(cardsOfBoardView(shown, view, 'party'))).toEqual(['Plan the party']);
  });

  it('keep a card the reader made on the visit, whatever the chips are, and not one somebody else made', () => {
    const shown = source(CARDS);
    // The visit starts as the cards are first read, before any chip is on.
    expect(filteredCardsOfBoardView(shown, view)).toHaveLength(3);
    const mine = { ...card('d', 'Fresh', {}), createdBy: 'me' };
    const theirs = card('e', 'Theirs', {});
    const later = source([...CARDS, mine, theirs]);
    // The same board store, its cards changed.
    Object.assign(shown, { realCards: later.realCards, cardsById: later.cardsById });
    quickFilters.toggle(boardId, 'overdue', 'visit');
    expect(titles(filteredCardsOfBoardView(shown, view))).toEqual(['Plan the launch', 'Fresh']);
    expect(mine.fields.properties).toEqual({});
  });

  it('take today from the clock for the due chips', () => {
    const shown = source([
      card('x', 'Due long ago', { due: { from: '2020-01-01' } }),
      card('y', 'Due far off', { due: { from: '2999-01-01' } }),
    ]);
    quickFilters.toggle(boardId, 'overdue', 'visit');
    expect(titles(cardsOfBoardView(shown, view))).toEqual(['Due long ago']);
  });
});
