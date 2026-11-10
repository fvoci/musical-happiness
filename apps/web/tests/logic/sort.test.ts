/**
 * The order of cards. The original has no tests for this code
 * (`store/cards.ts`); these were written for the port. The manual order and
 * the handling of several sort options differ from the original on purpose
 * (Q1 and Q6 in PORTING.md).
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { cardsOfView, hiddenCardCount } from '$lib/logic/card-list.ts';
import { calendarDate } from '$lib/logic/dates.ts';
import { diagnostics } from '$lib/logic/diagnostics.ts';
import { manualOrder, sortCards, titleOrCreatedOrder, type SortContext } from '$lib/logic/sort.ts';
import {
  createCard,
  createFilterGroup,
  formatDateValue,
  setCardProperty,
  TITLE_COLUMN_ID,
  type Card,
  type PropertyTemplate,
  type PropertyValue,
} from '$lib/logic/wire.ts';

function card(
  id: string,
  title: string,
  properties: Record<string, PropertyValue> = {},
  createAt = 1000,
): Card {
  const created = createCard();
  created.id = id;
  created.title = title;
  created.createAt = createAt;
  created.updateAt = createAt;
  for (const [propertyId, value] of Object.entries(properties)) {
    setCardProperty(created, propertyId, value);
  }
  return created;
}

const ids = (cards: readonly Card[]): string => cards.map((item) => item.id).join(' ');

const templates: PropertyTemplate[] = [
  { id: 'n', name: 'Number', type: 'number', options: [] },
  { id: 't', name: 'Text', type: 'text', options: [] },
  { id: 'd', name: 'Date', type: 'date', options: [] },
  {
    id: 's',
    name: 'Select',
    type: 'select',
    options: [
      { id: 'o1', value: 'zeta', color: '' },
      { id: 'o2', value: 'alpha', color: '' },
    ],
  },
  { id: 'who', name: 'Owner', type: 'person', options: [] },
  { id: 'people', name: 'People', type: 'multiPerson', options: [] },
  { id: 'by', name: 'Created by', type: 'createdBy', options: [] },
  { id: 'created', name: 'Created', type: 'createdTime', options: [] },
  { id: 'updated', name: 'Updated', type: 'updatedTime', options: [] },
];

const sortBy = (
  options: [propertyId: string, reversed?: boolean][],
  extra: Partial<SortContext> = {},
): SortContext => ({
  templates,
  sortOptions: options.map(([propertyId, reversed = false]) => ({ propertyId, reversed })),
  cardOrder: [],
  locale: 'en',
  timeZone: 'UTC',
  ...extra,
});

describe('titleOrCreatedOrder', () => {
  it('orders by title', () => {
    expect(titleOrCreatedOrder(card('a', 'apple'), card('b', 'banana'), 'en')).toBeLessThan(0);
  });

  it('puts a card without a title after one with a title', () => {
    expect(titleOrCreatedOrder(card('a', ''), card('b', 'banana'), 'en')).toBe(1);
    expect(titleOrCreatedOrder(card('a', 'apple'), card('b', ''), 'en')).toBe(-1);
  });

  it('orders two cards without a title by age, and by identifier when made at once', () => {
    expect(
      titleOrCreatedOrder(card('a', '', {}, 1000), card('b', '', {}, 2000), 'en'),
    ).toBeLessThan(0);
    expect(
      titleOrCreatedOrder(card('b', '', {}, 1000), card('a', '', {}, 1000), 'en'),
    ).toBeGreaterThan(0);
    expect(
      titleOrCreatedOrder(card('a', 'same', {}, 2000), card('b', 'same', {}, 1000), 'en'),
    ).toBeGreaterThan(0);
  });
});

describe('manual order', () => {
  const order = ['c', 'a', 'b'];
  const manual = (cards: Card[]): string =>
    ids(sortCards(cards, { ...sortBy([]), cardOrder: order }));

  it('follows the positions of the view', () => {
    expect(manual([card('a', '1'), card('b', '2'), card('c', '3')])).toBe('c a b');
  });

  it('puts cards without a position after all placed ones, oldest first', () => {
    const cards = [
      card('new2', 'aaa', {}, 3000),
      card('a', '1'),
      card('new1', 'zzz', {}, 2000),
      card('c', '3'),
    ];
    expect(manual(cards)).toBe('c a new1 new2');
  });

  it('gives opposite answers for the two ways round (Q1)', () => {
    const placed = card('a', '1');
    const unplaced = card('x', 'new');
    expect(manualOrder(order, placed, unplaced)).toBe(-1);
    expect(manualOrder(order, unplaced, placed)).toBe(1);
    expect(manualOrder(order, placed, placed)).toBe(0);
  });

  it('gives the same result whatever order the cards come in', () => {
    const cards = [
      card('x', 'q', {}, 5),
      card('a', '1'),
      card('y', 'p', {}, 4),
      card('b', '2'),
      card('c', '3'),
    ];
    const expected = 'c a b y x';
    expect(manual(cards)).toBe(expected);
    expect(manual([...cards].reverse())).toBe(expected);
    expect(manual([cards[2], cards[4], cards[0], cards[3], cards[1]] as Card[])).toBe(expected);
  });

  it('does not change the list it is given', () => {
    const cards = [card('a', '1'), card('b', '2'), card('c', '3')];
    manual(cards);
    expect(ids(cards)).toBe('a b c');
  });
});

describe('sorting by a property', () => {
  it('sorts by title; cards without one stay last both ways', () => {
    const cards = [card('b', 'banana'), card('a', 'apple'), card('c', '')];
    expect(ids(sortCards(cards, sortBy([[TITLE_COLUMN_ID]])))).toBe('a b c');
    expect(ids(sortCards(cards, sortBy([[TITLE_COLUMN_ID, true]])))).toBe('b a c');
  });

  it('sorts numbers as numbers', () => {
    const cards = [
      card('a', 'a', { n: '10' }),
      card('b', 'b', { n: '9' }),
      card('c', 'c', { n: '-1' }),
    ];
    expect(ids(sortCards(cards, sortBy([['n']])))).toBe('c b a');
    expect(ids(sortCards(cards, sortBy([['n', true]])))).toBe('a b c');
  });

  it('keeps cards without a value last whichever way the sort runs', () => {
    const cards = [
      card('empty', 'x'),
      card('a', 'a', { n: '1' }),
      card('b', 'b', { n: '2' }),
      card('text', 'y', { n: 'abc' }),
    ];
    expect(ids(sortCards(cards, sortBy([['n']])))).toBe('a b empty text');
    expect(ids(sortCards(cards, sortBy([['n', true]])))).toBe('b a empty text');
    const texts = [card('empty', 'x'), card('a', 'a', { t: 'one' }), card('b', 'b', { t: 'two' })];
    expect(ids(sortCards(texts, sortBy([['t', true]])))).toBe('b a empty');
  });

  it('breaks ties by title, then by age', () => {
    const cards = [
      card('b', 'banana', { t: 'same' }),
      card('a2', 'apple', { t: 'same' }, 2000),
      card('a1', 'apple', { t: 'same' }, 1000),
    ];
    expect(ids(sortCards(cards, sortBy([['t']])))).toBe('a1 a2 b');
  });

  it('sorts dates by their first day; a value without a time before one with a time that day', () => {
    const day = (date: number) => ({ date: calendarDate(2026, 3, date) });
    const cards = [
      card('late', 'x', { d: formatDateValue({ from: day(9) }) }),
      card('range', 'y', { d: formatDateValue({ from: day(5), to: day(20) }) }),
      card('timed', 'z', { d: formatDateValue({ from: { instant: Date.UTC(2026, 2, 5, 9, 0) } }) }),
      card('none', 'w'),
    ];
    expect(ids(sortCards(cards, sortBy([['d']], { timeZone: 'UTC' })))).toBe(
      'range timed late none',
    );
    expect(ids(sortCards(cards, sortBy([['d', true]], { timeZone: 'UTC' })))).toBe(
      'late timed range none',
    );
  });

  it('sorts a select by the label of its option, not by its identifier', () => {
    const cards = [
      card('zeta', 'x', { s: 'o1' }),
      card('alpha', 'y', { s: 'o2' }),
      card('gone', 'z', { s: 'deleted' }),
    ];
    expect(ids(sortCards(cards, sortBy([['s']])))).toBe('alpha zeta gone');
  });

  it('sorts people by username', () => {
    const usernameOf = (id: string): string | undefined => ({ u1: 'zoe', u2: 'adam' })[id];
    const people = [card('zoe', 'x', { people: ['u1'] }), card('adam', 'y', { people: ['u2'] })];
    expect(ids(sortCards(people, sortBy([['people']], { usernameOf })))).toBe('adam zoe');
    const owners = [card('zoe', 'x', { who: 'u1' }), card('adam', 'y', { who: 'u2' })];
    expect(ids(sortCards(owners, sortBy([['who']], { usernameOf })))).toBe('adam zoe');

    const first = card('zoe', 'x');
    first.createdBy = 'u1';
    const second = card('adam', 'y');
    second.createdBy = 'u2';
    expect(ids(sortCards([first, second], sortBy([['by']], { usernameOf })))).toBe('adam zoe');
  });

  it('sorts by creation time', () => {
    const cards = [card('newer', 'y', {}, 5000), card('older', 'x', {}, 1000)];
    expect(ids(sortCards(cards, sortBy([['created']])))).toBe('older newer');
    expect(ids(sortCards(cards, sortBy([['created', true]])))).toBe('newer older');
  });

  it('counts the newest comment as a change of the card', () => {
    const quiet = card('quiet', 'x');
    quiet.updateAt = 3000;
    const discussed = card('discussed', 'y');
    discussed.updateAt = 1000;
    const lastCommentAt = (id: string): number | undefined =>
      id === 'discussed' ? 9000 : undefined;
    expect(ids(sortCards([discussed, quiet], sortBy([['updated']])))).toBe('discussed quiet');
    expect(ids(sortCards([discussed, quiet], sortBy([['updated']], { lastCommentAt })))).toBe(
      'quiet discussed',
    );
  });
});

describe('several sort options (Q6)', () => {
  const cards = [
    card('a', 'a', { n: '1', t: 'y' }),
    card('b', 'b', { n: '2', t: 'x' }),
    card('c', 'c', { n: '1', t: 'x' }),
    card('d', 'd', { t: 'w' }),
  ];

  it('applies them in the order they are listed: the first decides, the next breaks its ties', () => {
    expect(ids(sortCards(cards, sortBy([['n'], ['t']])))).toBe('c a b d');
    expect(ids(sortCards(cards, sortBy([['t'], ['n']])))).toBe('d c b a');
  });

  it('turns each option on its own', () => {
    expect(ids(sortCards(cards, sortBy([['n', true], ['t']])))).toBe('b c a d');
    expect(ids(sortCards(cards, sortBy([['n'], ['t', true]])))).toBe('a c b d');
  });

  describe('an option on a property that no longer exists', () => {
    const report = vi.spyOn(diagnostics, 'report').mockImplementation(() => undefined);
    beforeEach(() => {
      report.mockClear();
    });

    it('is skipped and reported; the others still apply', () => {
      expect(ids(sortCards(cards, sortBy([['gone'], ['n'], ['t']])))).toBe('c a b d');
      expect(report).toHaveBeenCalledTimes(1);
    });
  });
});

describe('the cards of a view', () => {
  const view = {
    viewType: 'table' as const,
    filter: createFilterGroup({
      operation: 'and',
      filters: [{ propertyId: 't', condition: 'isNotEmpty', values: [] }],
    }),
    sortOptions: [{ propertyId: 'n', reversed: false }],
    cardOrder: [],
    visiblePropertyIds: [],
    dateDisplayPropertyId: undefined,
  };

  it('filters, searches and sorts, and leaves out withheld cards', () => {
    const withheld = card('withheld', 'needle', { t: 'x', n: '0' });
    withheld.limited = true;
    const cards = [
      card('second', 'needle two', { t: 'x', n: '2' }),
      card('filtered', 'needle', { n: '1' }),
      card('first', 'needle one', { t: 'x', n: '1' }),
      card('unmatched', 'hay', { t: 'x', n: '3' }),
      withheld,
    ];
    expect(
      ids(cardsOfView(cards, templates, view, 'NEEDLE', { locale: 'en', timeZone: 'UTC' })),
    ).toBe('first second');
    expect(ids(cardsOfView(cards, templates, view, '', { locale: 'en', timeZone: 'UTC' }))).toBe(
      'first second unmatched',
    );
    expect(hiddenCardCount(cards)).toBe(1);
  });
});
