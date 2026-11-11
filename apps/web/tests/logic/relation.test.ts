/**
 * The card relation kind outside of its editor: how its value is read and
 * written, and what sorting, filtering, searching, grouping, calculations
 * and a change of kind do with it. A card that is not there (deleted) counts
 * as not named, but its identifier stays in the stored value.
 */
import { describe, expect, it } from 'vitest';

import { planTypeChange } from '$lib/features/card/property-change.ts';
import { isClauseMet } from '$lib/logic/filter.ts';
import { propertyBehavior, SELECTABLE_PROPERTY_TYPES } from '$lib/logic/properties.ts';
import { searchFilterCards } from '$lib/logic/search.ts';
import { sortCards } from '$lib/logic/sort.ts';
import {
  createCard,
  parseRelationValue,
  setCardProperty,
  type Card,
  type FilterCondition,
  type PropertyTemplate,
  type PropertyValue,
} from '$lib/logic/wire.ts';

const RELATION: PropertyTemplate = { id: 'after', name: 'After', type: 'relation', options: [] };

function card(id: string, title: string, after?: PropertyValue): Card {
  const made = createCard();
  made.id = id;
  made.title = title;
  made.createAt = 1;
  if (after !== undefined) setCardProperty(made, 'after', after);
  return made;
}

const TITLES = new Map([
  ['a', 'Alpha'],
  ['b', 'Bravo'],
  ['c', 'Charlie'],
]);
const cardTitle = (id: string): string | undefined => TITLES.get(id);

describe('the stored value', () => {
  it('reads a list of card identifiers, each once, in order', () => {
    expect(parseRelationValue(['b', 'a', 'b', ''])).toEqual(['b', 'a']);
  });

  it('reads anything that is not a list as naming no card', () => {
    expect(parseRelationValue('a')).toEqual([]);
    expect(parseRelationValue({ from: '2026-01-01' })).toEqual([]);
    expect(parseRelationValue(undefined)).toEqual([]);
  });
});

describe('what the kind can do', () => {
  const behavior = propertyBehavior('relation');

  it('is offered when a property is added, and is no grouping and no calculation', () => {
    expect(SELECTABLE_PROPERTY_TYPES).toContain('relation');
    expect(behavior.canGroup).toBe(false);
    expect(behavior.calculationOptions).toEqual(['none']);
  });

  it('is filtered by whether it names a card that is there', () => {
    expect(behavior.filterValueType).toBe('presence');
    const met = (condition: FilterCondition, value?: PropertyValue): boolean =>
      isClauseMet(
        { propertyId: 'after', condition, values: [] },
        [RELATION],
        card('x', 'X', value),
        { timeZone: 'UTC', cardTitle },
      );
    expect(met('isNotEmpty', ['a'])).toBe(true);
    expect(met('isEmpty', ['a'])).toBe(false);
    expect(met('isEmpty')).toBe(true);
    // Only a deleted card, or the card itself: as good as empty.
    expect(met('isEmpty', ['gone'])).toBe(true);
    expect(met('isEmpty', ['x'])).toBe(true);
    expect(met('isNotEmpty', ['gone'])).toBe(false);
  });

  it('shows the titles of the cards that are there, for export', () => {
    const shown = behavior.displayValue(['c', 'gone', 'x', 'a'], card('x', 'X'), RELATION, {
      locale: 'en',
      timeZone: 'UTC',
      cardTitle,
    });
    expect(shown).toEqual(['Charlie', 'Alpha']);
  });

  it('sorts by the titles of the cards named; a card naming none, or only deleted ones, goes last', () => {
    const cards = [
      card('1', 'one', ['c']),
      card('2', 'two', ['gone']),
      card('3', 'three', ['a', 'c']),
      card('4', 'four'),
      card('5', 'five', ['b']),
    ];
    const sorted = (reversed: boolean): string[] =>
      sortCards(cards, {
        templates: [RELATION],
        sortOptions: [{ propertyId: 'after', reversed }],
        cardOrder: [],
        locale: 'en',
        timeZone: 'UTC',
        cardTitle,
      }).map((each) => each.title);
    expect(sorted(false)).toEqual(['three', 'five', 'one', 'four', 'two']);
    expect(sorted(true)).toEqual(['one', 'five', 'three', 'four', 'two']);
  });

  it('is not searched: its value is identifiers', () => {
    const cards = [card('1', 'one', ['bravo'])];
    expect(searchFilterCards(cards, [RELATION], 'bravo')).toEqual([]);
  });
});

describe('a change of kind', () => {
  it('to a relation removes the values of another kind', () => {
    const text = { ...RELATION, type: 'text' as const };
    const plan = planTypeChange(text, 'relation', [card('1', 'one', 'b'), card('2', 'two')]);
    expect(plan.lost).toBe(1);
    expect(plan.kept).toBe(0);
    expect([...plan.values]).toEqual([['1', undefined]]);
  });

  it('from a relation removes its values, whatever the new kind', () => {
    for (const type of ['text', 'multiSelect', 'multiPerson', 'date'] as const) {
      const plan = planTypeChange(RELATION, type, [card('1', 'one', ['a', 'b'])]);
      expect(plan.lost).toBe(1);
      expect([...plan.values]).toEqual([['1', undefined]]);
    }
  });

  it('from a list of people removes them: a person is no card', () => {
    const people = { ...RELATION, type: 'multiPerson' as const };
    expect(planTypeChange(people, 'relation', [card('1', 'one', ['user-1'])]).lost).toBe(1);
  });
});
