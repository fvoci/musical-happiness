/**
 * Behaviour of the filter that the original's tests do not reach and that
 * looks like a defect. Each case pins what the original does, so that the
 * port matches it until the behaviour is decided; the list is in PORTING.md.
 */
import { describe, expect, it } from 'vitest';

import { isClauseMet } from '$lib/logic/filter.ts';
import {
  createCard,
  createFilterClause,
  setCardProperty,
  type FilterCondition,
  type PropertyTemplate,
} from '$lib/logic/wire.ts';

const text: PropertyTemplate = { id: 'notes', name: 'Notes', type: 'text', options: [] };
const date: PropertyTemplate = { id: 'due', name: 'Due', type: 'date', options: [] };

function met(
  propertyId: string,
  condition: FilterCondition,
  value: string,
  cardValue: string,
): boolean {
  const card = createCard();
  card.title = cardValue;
  setCardProperty(card, 'notes', cardValue);
  setCardProperty(card, 'due', cardValue);
  const clause = createFilterClause({ propertyId, condition, values: [value] });
  return isClauseMet(clause, [text, date], card);
}

describe('quirk Q2: text conditions ignore case for the title only', () => {
  it('finds a title whatever its case', () => {
    expect(met('title', 'is', 'hello', 'Hello')).toBe(true);
    expect(met('title', 'contains', 'ELL', 'Hello')).toBe(true);
    expect(met('title', 'startsWith', 'HE', 'Hello')).toBe(true);
  });

  it('does not find a property value that has a capital letter', () => {
    expect(met('notes', 'is', 'Hello', 'Hello')).toBe(false);
    expect(met('notes', 'contains', 'Hell', 'Hello')).toBe(false);
    expect(met('notes', 'startsWith', 'He', 'Hello')).toBe(false);
    // Only an all-lower-case value can be found.
    expect(met('notes', 'is', 'HELLO', 'hello')).toBe(true);
    expect(met('notes', 'contains', 'ell', 'Hello')).toBe(true);
  });
});

describe('quirk Q5: "is" on a date compares to the millisecond', () => {
  const noon = Date.UTC(2026, 2, 5, 12);

  it('finds a day that is stored as exactly 12:00 UTC', () => {
    expect(met('due', 'is', String(noon), `{"from":${noon}}`)).toBe(true);
  });

  it('does not find the same day stored one minute off', () => {
    expect(met('due', 'is', String(noon), `{"from":${noon + 60_000}}`)).toBe(false);
  });

  it('finds a day inside a range, ends included', () => {
    const range = `{"from":${noon},"to":${noon + 2 * 86_400_000}}`;
    expect(met('due', 'is', String(noon + 86_400_000), range)).toBe(true);
    expect(met('due', 'is', String(noon + 2 * 86_400_000), range)).toBe(true);
    expect(met('due', 'is', String(noon + 3 * 86_400_000), range)).toBe(false);
  });
});

describe('quirk Q16: the day of a creation time filter is a UTC day', () => {
  const created: PropertyTemplate = {
    id: 'created',
    name: 'Created',
    type: 'createdTime',
    options: [],
  };
  // What the date picker of the filter stores for 7 December 2023: its noon in UTC.
  const seventh = String(Date.UTC(2023, 11, 7, 12));

  function createdAt(instant: number, condition: FilterCondition): boolean {
    const card = createCard();
    card.createAt = instant;
    const clause = createFilterClause({ propertyId: 'created', condition, values: [seventh] });
    return isClauseMet(clause, [created], card);
  }

  it('takes a card for the 7th when it was created between 00:00 and 24:00 UTC of the 7th', () => {
    expect(createdAt(Date.UTC(2023, 11, 7, 0, 1), 'is')).toBe(true);
    expect(createdAt(Date.UTC(2023, 11, 7, 23, 59), 'is')).toBe(true);
  });

  it('so in Seoul a card made at 03:00 on the 7th is not "the 7th", and one made at 05:00 on the 8th is', () => {
    const threeAmOnThe7thInSeoul = Date.UTC(2023, 11, 6, 18, 0);
    const fiveAmOnThe8thInSeoul = Date.UTC(2023, 11, 7, 20, 0);
    expect(createdAt(threeAmOnThe7thInSeoul, 'is')).toBe(false);
    expect(createdAt(threeAmOnThe7thInSeoul, 'isBefore')).toBe(true);
    expect(createdAt(fiveAmOnThe8thInSeoul, 'is')).toBe(true);
    expect(createdAt(fiveAmOnThe8thInSeoul, 'isAfter')).toBe(false);
  });
});
