/** The summary row of a table and the summary of a board column. */
import {
  displayDate,
  displayDateTime,
  humanizeDuration,
  roundTo,
  type DisplayContext,
} from './format.ts';
import {
  cardProperty,
  parseDateObject,
  TITLE_COLUMN_ID,
  type Card,
  type PropertyTemplate,
} from './wire.ts';

const DECIMALS = 2;

export const CALCULATION_IDS = [
  'count',
  'countEmpty',
  'countNotEmpty',
  'percentEmpty',
  'percentNotEmpty',
  'countValue',
  'countUniqueValue',
  'countChecked',
  'countUnchecked',
  'percentChecked',
  'percentUnchecked',
  'sum',
  'average',
  'median',
  'min',
  'max',
  'range',
  'earliest',
  'latest',
  'dateRange',
] as const;

export type CalculationId = (typeof CALCULATION_IDS)[number];

export type Calculation = (
  cards: readonly Card[],
  property: PropertyTemplate,
  context: DisplayContext,
) => string;

/** Times that are shown to the minute count as equal within the minute. */
const toTheMinute = (timestamp: number): number => timestamp - (timestamp % 60000);

function valueOf(card: Card, property: PropertyTemplate): string | string[] | number | undefined {
  if (property.id === TITLE_COLUMN_ID) return card.title;
  switch (property.type) {
    case 'createdBy':
      return card.createdBy;
    case 'createdTime':
      return toTheMinute(card.createAt);
    case 'updatedBy':
      return card.modifiedBy;
    case 'updatedTime':
      return toTheMinute(card.updateAt);
    default:
      return cardProperty(card, property.id);
  }
}

/** The numeric reading of a card's value; NaN when it is not a number. */
const numberOf = (card: Card, property: PropertyTemplate): number =>
  Number.parseFloat(String(valueOf(card, property)));

const withValue = (cards: readonly Card[], property: PropertyTemplate): Card[] =>
  cards.filter((card) => Boolean(valueOf(card, property)));

const percent = (part: number, whole: number): string => `${((part / whole) * 100).toFixed(0)}%`;

/** How many values there are: every item of a multi-select counts. */
function valueCount(cards: readonly Card[], property: PropertyTemplate): number {
  const filled = withValue(cards, property);
  if (property.type !== 'multiSelect') return filled.length;
  return filled.reduce((sum, card) => sum + (valueOf(card, property) as string[]).length, 0);
}

function extremes(
  cards: readonly Card[],
  property: PropertyTemplate,
): { low: number; high: number } {
  let low = Number.POSITIVE_INFINITY;
  let high = Number.NEGATIVE_INFINITY;
  for (const card of withValue(cards, property)) {
    const value = numberOf(card, property);
    low = Math.min(low, value);
    high = Math.max(high, value);
  }
  return { low, high };
}

/** The text for a minimum or maximum; `0` when there was no value at all (`none`). */
const rounded = (value: number, none: number): string =>
  value === none ? '0' : String(roundTo(value, DECIMALS));
const lowest = (value: number): string => rounded(value, Number.POSITIVE_INFINITY);
const highest = (value: number): string => rounded(value, Number.NEGATIVE_INFINITY);

function timestamps(card: Card, property: PropertyTemplate): number[] {
  const value = valueOf(card, property);
  if (typeof value === 'number') return [value];
  if (typeof value !== 'string') return [];
  const { from, to } = parseDateObject(value);
  return [from, to].filter((time): time is number => Boolean(time));
}

function timeSpan(
  cards: readonly Card[],
  property: PropertyTemplate,
): { first: number; last: number } {
  let first = Number.POSITIVE_INFINITY;
  let last = Number.NEGATIVE_INFINITY;
  for (const card of cards) {
    for (const time of timestamps(card, property)) {
      first = Math.min(first, time);
      last = Math.max(last, time);
    }
  }
  return { first, last };
}

const showTime = (
  timestamp: number,
  property: PropertyTemplate,
  context: DisplayContext,
): string => {
  if (!Number.isFinite(timestamp)) return '';
  const date = new Date(timestamp);
  return property.type === 'date'
    ? displayDate(date, context.locale, context.now)
    : displayDateTime(date, context.locale, context.now);
};

function sumOf(cards: readonly Card[], property: PropertyTemplate): number {
  return withValue(cards, property).reduce((sum, card) => sum + numberOf(card, property), 0);
}

function median(cards: readonly Card[], property: PropertyTemplate): string {
  const values = withValue(cards, property)
    .map((card) => numberOf(card, property))
    .sort((a, b) => a - b);
  if (values.length === 0) return '0';
  const middle = Math.floor(values.length / 2);
  const upper = values[middle] ?? 0;
  const result = values.length % 2 === 0 ? (upper + (values[middle - 1] ?? 0)) / 2 : upper;
  return String(roundTo(result, DECIMALS));
}

/**
 * Every calculation by its identifier. Each returns the text to show.
 *
 * QUIRK(Q9): the checkbox calculations count a card as checked when its
 * value is any non-empty text, so a stored `false` counts as checked. The
 * editor of the original writes `true` or an empty text, never `false`, but
 * imported data can hold it. Kept as in the original.
 */
export const CALCULATIONS: Record<CalculationId, Calculation> = {
  count: (cards) => String(cards.length),
  countEmpty: (cards, property) => String(cards.length - withValue(cards, property).length),
  countNotEmpty: (cards, property) => String(withValue(cards, property).length),
  percentEmpty: (cards, property) =>
    cards.length === 0
      ? ''
      : percent(cards.length - withValue(cards, property).length, cards.length),
  percentNotEmpty: (cards, property) =>
    cards.length === 0 ? '' : percent(withValue(cards, property).length, cards.length),
  countValue: (cards, property) => String(valueCount(cards, property)),
  countUniqueValue: (cards, property) => {
    const unique = new Set<string>();
    for (const card of cards) {
      const value = valueOf(card, property);
      if (!value) continue;
      if (property.type === 'multiSelect') for (const item of value as string[]) unique.add(item);
      else unique.add(String(value));
    }
    return String(unique.size);
  },
  countChecked: (cards, property) => String(valueCount(cards, property)),
  countUnchecked: (cards, property) => String(cards.length - valueCount(cards, property)),
  percentChecked: (cards, property) =>
    `${Math.round((valueCount(cards, property) * 100) / cards.length)}%`,
  percentUnchecked: (cards, property) =>
    `${Math.round(((cards.length - valueCount(cards, property)) * 100) / cards.length)}%`,
  sum: (cards, property) => String(roundTo(sumOf(cards, property), DECIMALS)),
  average: (cards, property) => {
    const count = withValue(cards, property).length;
    if (count === 0) return '0';
    return String(roundTo(roundTo(sumOf(cards, property), DECIMALS) / count, DECIMALS));
  },
  median,
  min: (cards, property) => lowest(extremes(cards, property).low),
  max: (cards, property) => highest(extremes(cards, property).high),
  range: (cards, property) => {
    const { low, high } = extremes(cards, property);
    return `${lowest(low)} - ${highest(high)}`;
  },
  earliest: (cards, property, context) =>
    showTime(timeSpan(cards, property).first, property, context),
  latest: (cards, property, context) => showTime(timeSpan(cards, property).last, property, context),
  dateRange: (cards, property, context) => {
    const { first, last } = timeSpan(cards, property);
    return Number.isFinite(first) ? humanizeDuration(last - first, context.locale) : '';
  },
};
