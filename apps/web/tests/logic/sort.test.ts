/**
 * The order of cards. The original has no tests for this code
 * (`store/cards.ts`); these were written for the port. Cases named
 * "quirk Qn" pin a behaviour of the original that looks like a defect and
 * is listed in PORTING.md; they are to change when the behaviour is decided.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { cardsOfView, hiddenCardCount } from '$lib/logic/card-list.ts';
import { diagnostics } from '$lib/logic/diagnostics.ts';
import { manualOrder, sortCards, titleOrCreatedOrder, type SortContext } from '$lib/logic/sort.ts';
import {
  createCard,
  createFilterGroup,
  setCardProperty,
  TITLE_COLUMN_ID,
  type Card,
  type PropertyTemplate,
  type PropertyValue,
} from '$lib/logic/wire.ts';

function card(id: string, title: string, properties: Record<string, PropertyValue> = {}): Card {
  const created = createCard();
  created.id = id;
  created.title = title;
  created.createAt = 1000;
  created.updateAt = 1000;
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
  { id: 'people', name: 'People', type: 'multiPerson', options: [] },
  { id: 'by', name: 'Created by', type: 'createdBy', options: [] },
  { id: 'created', name: 'Created', type: 'createdTime', options: [] },
  { id: 'updated', name: 'Updated', type: 'updatedTime', options: [] },
];

const sortBy = (
  propertyId: string,
  reversed = false,
  extra: Partial<SortContext> = {},
): SortContext => ({
  templates,
  sortOptions: [{ propertyId, reversed }],
  cardOrder: [],
  ...extra,
});

describe('titleOrCreatedOrder', () => {
  it('orders by title', () => {
    expect(titleOrCreatedOrder(card('a', 'apple'), card('b', 'banana'))).toBeLessThan(0);
  });

  it('puts a card without a title after one with a title', () => {
    expect(titleOrCreatedOrder(card('a', ''), card('b', 'banana'))).toBe(1);
    expect(titleOrCreatedOrder(card('a', 'apple'), card('b', ''))).toBe(-1);
  });

  it('orders two cards without a title by age', () => {
    const older = card('a', '');
    const newer = card('b', '');
    newer.createAt = 2000;
    expect(titleOrCreatedOrder(older, newer)).toBeLessThan(0);
  });
});

describe('manual order', () => {
  const order = ['c', 'a', 'b'];

  it('follows the positions of the view', () => {
    const cards = [card('a', '1'), card('b', '2'), card('c', '3')];
    expect(ids(sortCards(cards, { templates, sortOptions: [], cardOrder: order }))).toBe('c a b');
  });

  it('orders two cards without a position by title', () => {
    expect(manualOrder(order, card('x', 'apple'), card('y', 'banana'))).toBeLessThan(0);
  });

  it('puts a card without a position after a placed one', () => {
    expect(manualOrder(order, card('x', 'new'), card('a', '1'))).toBe(1);
  });

  it('quirk Q1: a placed card compared with one without a position is also "after"', () => {
    // The mirror of the case above should be negative. It is the position plus one.
    expect(manualOrder(order, card('a', '1'), card('x', 'new'))).toBe(2);
    expect(manualOrder(order, card('c', '3'), card('x', 'new'))).toBe(1);
  });

  it('does not change the list it is given', () => {
    const cards = [card('a', '1'), card('b', '2'), card('c', '3')];
    sortCards(cards, { templates, sortOptions: [], cardOrder: order });
    expect(ids(cards)).toBe('a b c');
  });
});

describe('sorting by a property', () => {
  it('sorts by title, both ways', () => {
    const cards = [card('b', 'banana'), card('a', 'apple'), card('c', '')];
    expect(ids(sortCards(cards, sortBy(TITLE_COLUMN_ID)))).toBe('a b c');
    expect(ids(sortCards(cards, sortBy(TITLE_COLUMN_ID, true)))).toBe('c b a');
  });

  it('sorts numbers as numbers', () => {
    const cards = [
      card('a', 'a', { n: '10' }),
      card('b', 'b', { n: '9' }),
      card('c', 'c', { n: '-1' }),
    ];
    expect(ids(sortCards(cards, sortBy('n')))).toBe('c b a');
    expect(ids(sortCards(cards, sortBy('n', true)))).toBe('a b c');
  });

  it('keeps cards without a value last whichever way the sort runs', () => {
    const cards = [card('empty', 'x'), card('a', 'a', { n: '1' }), card('b', 'b', { n: '2' })];
    expect(ids(sortCards(cards, sortBy('n')))).toBe('a b empty');
    expect(ids(sortCards(cards, sortBy('n', true)))).toBe('b a empty');
    const texts = [card('empty', 'x'), card('a', 'a', { t: 'one' }), card('b', 'b', { t: 'two' })];
    expect(ids(sortCards(texts, sortBy('t', true)))).toBe('b a empty');
  });

  it('breaks ties by title', () => {
    const cards = [card('b', 'banana', { t: 'same' }), card('a', 'apple', { t: 'same' })];
    expect(ids(sortCards(cards, sortBy('t')))).toBe('a b');
  });

  it('sorts dates by their start', () => {
    const cards = [
      card('late', 'x', { d: '{"from":2000}' }),
      card('early', 'y', { d: '{"from":1000,"to":9000}' }),
    ];
    expect(ids(sortCards(cards, sortBy('d')))).toBe('early late');
  });

  it('quirk Q10: a date written as a bare number sorts as empty', () => {
    const cards = [card('bare', 'x', { d: '1000' }), card('object', 'y', { d: '{"from":2000}' })];
    expect(ids(sortCards(cards, sortBy('d')))).toBe('object bare');
  });

  it('sorts a select by the label of its option, not by its identifier', () => {
    const cards = [card('zeta', 'x', { s: 'o1' }), card('alpha', 'y', { s: 'o2' })];
    expect(ids(sortCards(cards, sortBy('s')))).toBe('alpha zeta');
  });

  it('sorts people by username', () => {
    const usernameOf = (id: string): string | undefined => ({ u1: 'zoe', u2: 'adam' })[id];
    const cards = [card('zoe', 'x', { people: ['u1'] }), card('adam', 'y', { people: ['u2'] })];
    expect(ids(sortCards(cards, sortBy('people', false, { usernameOf })))).toBe('adam zoe');

    const first = card('zoe', 'x');
    first.createdBy = 'u1';
    const second = card('adam', 'y');
    second.createdBy = 'u2';
    expect(ids(sortCards([first, second], sortBy('by', false, { usernameOf })))).toBe('adam zoe');
  });

  it('sorts by creation time', () => {
    const older = card('older', 'x');
    const newer = card('newer', 'y');
    newer.createAt = 5000;
    expect(ids(sortCards([newer, older], sortBy('created')))).toBe('older newer');
    expect(ids(sortCards([older, newer], sortBy('created', true)))).toBe('newer older');
  });

  it('counts the newest comment as a change of the card', () => {
    const quiet = card('quiet', 'x');
    quiet.updateAt = 3000;
    const discussed = card('discussed', 'y');
    discussed.updateAt = 1000;
    const lastCommentAt = (id: string): number | undefined =>
      id === 'discussed' ? 9000 : undefined;
    expect(ids(sortCards([discussed, quiet], sortBy('updated')))).toBe('discussed quiet');
    expect(ids(sortCards([discussed, quiet], sortBy('updated', false, { lastCommentAt })))).toBe(
      'quiet discussed',
    );
  });

  describe('a sort option on a property that no longer exists', () => {
    const report = vi.spyOn(diagnostics, 'report').mockImplementation(() => undefined);
    beforeEach(() => {
      report.mockClear();
    });

    it('leaves the order alone and reports it', () => {
      const cards = [card('b', 'banana'), card('a', 'apple')];
      expect(ids(sortCards(cards, sortBy('gone')))).toBe('b a');
      expect(report).toHaveBeenCalledTimes(1);
    });
  });

  it('quirk Q6: with two sort options only the last one shows', () => {
    const cards = [
      card('a', 'a', { n: '1', t: 'x' }),
      card('b', 'b', { n: '2', t: 'x' }),
      card('c', 'c', { n: '1', t: 'y' }),
    ];
    // By number, then by text: one would expect "a c b" (number first, text within).
    const context: SortContext = {
      templates,
      sortOptions: [
        { propertyId: 'n', reversed: false },
        { propertyId: 't', reversed: false },
      ],
      cardOrder: [],
    };
    // The text sort runs last over everything and breaks its ties by title.
    expect(ids(sortCards(cards, context))).toBe('a b c');
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
    expect(ids(cardsOfView(cards, templates, view, 'NEEDLE'))).toBe('first second');
    expect(ids(cardsOfView(cards, templates, view, ''))).toBe('first second unmatched');
    expect(hiddenCardCount(cards)).toBe(1);
  });
});
