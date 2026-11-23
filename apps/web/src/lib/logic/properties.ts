/**
 * What each kind of card property can do.
 *
 * A property template names its kind in `type`; this table says, per kind,
 * whether it can group and filter, which calculations apply, and how a value
 * of it is shown and exported. Code that needs to know about a kind looks it
 * up here instead of switching on the name.
 */
import type { CalculationId } from './calculations.ts';
import { cardUpdatedAt } from './card-time.ts';
import { dateOfInstant, daysOf, type CalendarDate, type DatePoint } from './dates.ts';
import { diagnostics } from './diagnostics.ts';
import { displayCalendarDate, displayDateTime, type DisplayContext } from './format.ts';
import {
  parseDateValue,
  plainPropertyValue,
  relationCardIds,
  type Card,
  type PropertyTemplate,
  type PropertyType,
  type PropertyValue,
} from './wire.ts';

/** Which kind of control edits the value of a filter on this property. */
export type FilterValueType =
  | 'none'
  | 'options'
  | 'boolean'
  | 'text'
  | 'date'
  | 'person'
  /** Only whether there is a value: "is not empty", "is empty". */
  | 'presence';

export type CalculationOption = 'none' | CalculationId;

type Display = (
  value: PropertyValue | undefined,
  card: Card,
  template: PropertyTemplate,
  context: DisplayContext,
) => string | string[] | undefined;

export interface PropertyBehavior {
  canGroup: boolean;
  canFilter: boolean;
  filterValueType: FilterValueType;
  /** The value comes from the card itself and cannot be edited. */
  isReadOnly: boolean;
  calculationOptions: readonly CalculationOption[];
  /** The text shown for a value; a list for several options. */
  displayValue: Display;
  /** The value as one cell of a CSV file, quoted where needed. */
  exportValue: (
    value: PropertyValue | undefined,
    card: Card,
    template: PropertyTemplate,
    context: DisplayContext,
  ) => string;
  /**
   * For the date kinds: the first and last day a card occupies in a calendar,
   * as the reader's calendar has them.
   */
  days?: (
    value: PropertyValue | undefined,
    card: Card,
    timeZone: string,
  ) => { first: CalendarDate; last: CalendarDate } | undefined;
}

const COMMON_CALCULATIONS: readonly CalculationOption[] = [
  'none',
  'count',
  'countEmpty',
  'countNotEmpty',
  'percentEmpty',
  'percentNotEmpty',
  'countValue',
  'countUniqueValue',
];
const NUMBER_CALCULATIONS = [
  ...COMMON_CALCULATIONS,
  'sum',
  'average',
  'median',
  'min',
  'max',
  'range',
] as const;
const TIME_CALCULATIONS = [...COMMON_CALCULATIONS, 'earliest', 'latest', 'dateRange'] as const;

/** Marks the `#` of a cell; the code that builds the download link turns it into `%23`. */
export const HASH_SIGN_TOKEN = '___hash_sign___';

/** The text of a CSV cell: quotes doubled, `#` replaced by its token. */
export function encodeCsvText(text: string): string {
  return text.replace(/"/g, '""').replace(/#/g, HASH_SIGN_TOKEN);
}

function exportDisplayed(display: Display): PropertyBehavior['exportValue'] {
  return (value, card, template, context) => {
    const shown = display(value, card, template, context);
    if (typeof shown === 'string') return `"${encodeCsvText(shown)}"`;
    if (Array.isArray(shown)) return `"${encodeCsvText(shown.join('|'))}"`;
    return '';
  };
}

function behavior(overrides: Partial<PropertyBehavior>): PropertyBehavior {
  const displayValue: Display = overrides.displayValue ?? plainPropertyValue;
  return {
    canGroup: false,
    canFilter: false,
    filterValueType: 'none',
    isReadOnly: false,
    calculationOptions: COMMON_CALCULATIONS,
    exportValue: exportDisplayed(displayValue),
    ...overrides,
    displayValue,
  };
}

const textLike = behavior({ canFilter: true, filterValueType: 'text' });

const displaySelect: Display = (stored, card, template, context) => {
  const value = plainPropertyValue(stored);
  if (!value) return '';
  const option = template.options.find((candidate) => candidate.id === value);
  if (!option) {
    diagnostics.report(`invalid select option ${String(value)} on card "${card.title}"`);
  }
  return option?.value || (context.unknownOption ?? '(Unknown)');
};

const displayMultiSelect: Display = (stored, card, template) => {
  const value = plainPropertyValue(stored);
  if (!value?.length) return '';
  const options = template.options.filter((option) => value.includes(option.id));
  if (options.length === 0) {
    diagnostics.report(`invalid multi-select options ${String(value)} on card "${card.title}"`);
  }
  return options.map((option) => option.value);
};

/**
 * The titles of the cards a relation names. A card that is not there
 * (deleted) is left out; without a way to look titles up, the identifiers.
 */
const displayRelation: Display = (value, card, _template, context) => {
  const ids = relationCardIds(value).filter((id) => id !== card.id);
  const { cardTitle } = context;
  if (!cardTitle) return ids;
  // An untitled card adds no text, as in a sort.
  return ids.flatMap((id) => cardTitle(id) || []);
};

/** A day as a date, a moment as a date with its time. */
function displayPoint(point: DatePoint, context: DisplayContext): string {
  return point.instant === undefined
    ? displayCalendarDate(point.date, context)
    : displayDateTime(point.instant, context);
}

const displayDateValue: Display = (value, _card, _template, context) => {
  const { from, to } = parseDateValue(value);
  if (!from) return '';
  return to
    ? `${displayPoint(from, context)} -> ${displayPoint(to, context)}`
    : displayPoint(from, context);
};

/** The day a moment of the card itself falls on, as a one-day range. */
const dayOfMoment = (
  instant: number,
  timeZone: string,
): { first: CalendarDate; last: CalendarDate } => {
  const day = dateOfInstant(instant, timeZone);
  return { first: day, last: day };
};

/** The behaviour of every kind of property. `file` has none of its own and behaves as unknown. */
export const PROPERTY_BEHAVIORS: Record<PropertyType, PropertyBehavior> = {
  text: textLike,
  url: textLike,
  email: textLike,
  phone: textLike,
  number: behavior({
    calculationOptions: NUMBER_CALCULATIONS,
    exportValue: (value) => (value ? Number(value).toString() : ''),
  }),
  select: behavior({
    canGroup: true,
    canFilter: true,
    filterValueType: 'options',
    displayValue: displaySelect,
  }),
  multiSelect: behavior({
    canFilter: true,
    filterValueType: 'options',
    displayValue: displayMultiSelect,
    // Not quoted, unlike every other kind.
    exportValue: (value, card, template, context) => {
      const shown = displayMultiSelect(value, card, template, context);
      return Array.isArray(shown) ? shown.join('|') : '';
    },
  }),
  date: behavior({
    canFilter: true,
    filterValueType: 'date',
    displayValue: displayDateValue,
    days: (value, _card, timeZone) => daysOf(parseDateValue(value), timeZone),
  }),
  person: behavior({ canGroup: true, canFilter: true, filterValueType: 'person' }),
  multiPerson: behavior({ canFilter: true, filterValueType: 'person' }),
  checkbox: behavior({ canFilter: true, filterValueType: 'boolean' }),
  createdTime: behavior({
    canFilter: true,
    filterValueType: 'date',
    isReadOnly: true,
    calculationOptions: TIME_CALCULATIONS,
    displayValue: (_value, card, _template, context) => displayDateTime(card.createAt, context),
    days: (_value, card, timeZone) => dayOfMoment(card.createAt, timeZone),
  }),
  updatedTime: behavior({
    canFilter: true,
    filterValueType: 'date',
    isReadOnly: true,
    calculationOptions: TIME_CALCULATIONS,
    displayValue: (_value, card, _template, context) =>
      displayDateTime(cardUpdatedAt(card), context),
    days: (_value, card, timeZone) => dayOfMoment(cardUpdatedAt(card), timeZone),
  }),
  createdBy: behavior({
    canGroup: true,
    canFilter: true,
    filterValueType: 'person',
    isReadOnly: true,
  }),
  updatedBy: behavior({
    canGroup: true,
    canFilter: true,
    filterValueType: 'person',
    isReadOnly: true,
  }),
  // Not grouped by and not calculated; a filter asks only whether there is a value.
  relation: behavior({
    canFilter: true,
    filterValueType: 'presence',
    calculationOptions: ['none'],
    displayValue: displayRelation,
  }),
  file: behavior({}),
  unknown: behavior({}),
};

/** The behaviour of a kind; a kind this app does not know behaves as `unknown`. */
export function propertyBehavior(type: string): PropertyBehavior {
  return (
    (PROPERTY_BEHAVIORS as Record<string, PropertyBehavior | undefined>)[type] ??
    PROPERTY_BEHAVIORS.unknown
  );
}

/** The kinds a person can choose when adding a property, in menu order. */
export const SELECTABLE_PROPERTY_TYPES: readonly PropertyType[] = [
  'text',
  'number',
  'email',
  'phone',
  'url',
  'select',
  'multiSelect',
  'date',
  'person',
  'multiPerson',
  'checkbox',
  'createdTime',
  'createdBy',
  'updatedTime',
  'updatedBy',
  'relation',
];
