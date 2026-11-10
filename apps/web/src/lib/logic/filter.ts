/** Deciding which cards a view's filter lets through. */
import { arraysEqual } from './arrays.ts';
import { diagnostics } from './diagnostics.ts';
import {
  cardProperty,
  isFilterGroup,
  parseDateValue,
  type Card,
  type DateValue,
  type FilterClause,
  type FilterGroup,
  type PropertyTemplate,
  type PropertyValue,
} from './wire.ts';

const HALF_DAY = 12 * 60 * 60 * 1000;

/** Whether two clauses filter the same property in the same way. */
export function areEqual(first: FilterClause, second: FilterClause): boolean {
  return (
    first.propertyId === second.propertyId &&
    first.condition === second.condition &&
    arraysEqual(first.values, second.values)
  );
}

export function applyFilterGroup(
  group: FilterGroup,
  templates: readonly PropertyTemplate[],
  cards: readonly Card[],
): Card[] {
  return cards.filter((card) => isFilterGroupMet(group, templates, card));
}

/** A group without filters lets every card through. */
export function isFilterGroupMet(
  group: FilterGroup,
  templates: readonly PropertyTemplate[],
  card: Card,
): boolean {
  if (group.filters.length < 1) return true;
  const met = (filter: FilterClause | FilterGroup): boolean =>
    isFilterGroup(filter)
      ? isFilterGroupMet(filter, templates, card)
      : isClauseMet(filter, templates, card);
  return group.operation === 'or' ? group.filters.some(met) : group.filters.every(met);
}

const isTimeOfCard = (template: PropertyTemplate | undefined): boolean =>
  template?.type === 'createdTime' || template?.type === 'updatedTime';

/**
 * The value a clause is compared with, and its reading as a date for date
 * properties. A property without a stored value falls back to what the card
 * itself knows: who created or changed it, and when.
 */
function valueForClause(
  filter: FilterClause,
  template: PropertyTemplate | undefined,
  card: Card,
): { value: PropertyValue | undefined; dateValue: DateValue | undefined } {
  let value = cardProperty(card, filter.propertyId);
  if (filter.propertyId === 'title') value = card.title.toLowerCase();

  let dateValue: DateValue | undefined;
  if (template?.type === 'date') dateValue = parseDateValue(value as string | undefined);

  if (!value && template) {
    if (template.type === 'createdBy') {
      value = card.createdBy;
    } else if (template.type === 'updatedBy') {
      value = card.modifiedBy;
    } else if (template.type === 'createdTime') {
      value = card.createAt.toString();
      dateValue = parseDateValue(value);
    } else if (template.type === 'updatedTime') {
      value = card.updateAt.toString();
      dateValue = parseDateValue(value);
    }
  }
  return { value, dateValue };
}

/**
 * Whether a card meets one clause.
 *
 * A clause that cannot be evaluated (no values to compare with, an unknown
 * condition) is met: an unfinished filter must not hide every card.
 *
 * QUIRK(Q2): the text conditions lower-case the value of the filter but only
 * the title of the card, not a property value. A filter "is hello" finds the
 * title "Hello" and never finds the property value "Hello". Kept as in the
 * original.
 */
export function isClauseMet(
  filter: FilterClause,
  templates: readonly PropertyTemplate[],
  card: Card,
): boolean {
  const template = templates.find((candidate) => candidate.id === filter.propertyId);
  const { value, dateValue } = valueForClause(filter, template, card);
  const first = filter.values[0];
  const needle = first?.toLowerCase() ?? '';
  const text = typeof value === 'string' ? value : '';
  // Text or a list; anything else a stored value might be counts as empty.
  const stored = typeof value === 'string' || Array.isArray(value) ? value : '';
  const hasAnyValue = (): boolean =>
    filter.values.some((candidate) =>
      Array.isArray(value) ? value.includes(candidate) : candidate === value,
    );

  switch (filter.condition) {
    case 'includes':
      return filter.values.length < 1 || hasAnyValue();
    case 'notIncludes':
      return filter.values.length < 1 || !hasAnyValue();
    case 'isEmpty':
      return stored.length <= 0;
    case 'isNotEmpty':
      return stored.length > 0;
    case 'isSet':
      return Boolean(value);
    case 'isNotSet':
      return !value;
    case 'is': {
      if (first === undefined) return true;
      if (dateValue === undefined) return needle === value;
      const day = Number.parseInt(first, 10);
      if (isTimeOfCard(template)) {
        // A time of day, compared with a day: anything within twelve hours of its noon.
        return dateValue.from
          ? dateValue.from > day - HALF_DAY && dateValue.from < day + HALF_DAY
          : false;
      }
      if (dateValue.from && dateValue.to) return dateValue.from <= day && dateValue.to >= day;
      // QUIRK(Q5): equal to the millisecond. It holds because both sides are
      // written as 12:00 UTC of their day.
      return dateValue.from === day;
    }
    case 'contains':
      // A list value (several options) contains the text when one item equals it.
      return first === undefined || stored.includes(needle);
    case 'notContains':
      return first === undefined || !stored.includes(needle);
    case 'startsWith':
      return first === undefined || text.startsWith(needle);
    case 'notStartsWith':
      return first === undefined || !text.startsWith(needle);
    case 'endsWith':
      return first === undefined || text.endsWith(needle);
    case 'notEndsWith':
      return first === undefined || !text.endsWith(needle);
    case 'isBefore': {
      if (first === undefined) return true;
      if (dateValue === undefined) return false;
      const day = Number.parseInt(first, 10);
      if (isTimeOfCard(template)) return dateValue.from ? dateValue.from < day - HALF_DAY : false;
      return dateValue.from ? dateValue.from < day : false;
    }
    case 'isAfter': {
      if (first === undefined) return true;
      if (dateValue === undefined) return false;
      const day = Number.parseInt(first, 10);
      if (isTimeOfCard(template)) return dateValue.from ? dateValue.from > day + HALF_DAY : false;
      if (dateValue.to) return dateValue.to > day;
      return dateValue.from ? dateValue.from > day : false;
    }
    default:
      diagnostics.report(`invalid filter condition: ${String(filter.condition)}`);
      return true;
  }
}

/**
 * Property values a new card needs in order to pass the filter, so that a
 * card added to a filtered view does not vanish at once. Only the clauses
 * directly in the group are looked at, and of an `or` group only the first.
 */
export function propertiesThatMeetFilterGroup(
  group: FilterGroup | undefined,
  templates: readonly PropertyTemplate[],
): Record<string, string> {
  if (!group) return {};
  const clauses = group.filters.filter((filter): filter is FilterClause => !isFilterGroup(filter));
  const considered = group.operation === 'or' ? clauses.slice(0, 1) : clauses;
  const result: Record<string, string> = {};
  for (const clause of considered) {
    const property = propertyThatMeetsFilterClause(clause, templates);
    if (property.value) result[property.id] = property.value;
  }
  return result;
}

/** The value that makes a card meet the clause, when there is an obvious one. */
export function propertyThatMeetsFilterClause(
  clause: FilterClause,
  templates: readonly PropertyTemplate[],
): { id: string; value?: string } {
  const id = clause.propertyId;
  const template = templates.find((candidate) => candidate.id === id);
  if (!template) {
    diagnostics.report(`propertyThatMeetsFilterClause: no template for property ${id}`);
    return { id };
  }
  if (template.type === 'createdBy' || template.type === 'updatedBy') return { id };

  if (clause.condition === 'includes') {
    const first = clause.values[0];
    return first === undefined ? { id } : { id, value: first };
  }
  if (clause.condition === 'isNotEmpty' && template.type === 'select') {
    const option = template.options[0];
    return option ? { id, value: option.id } : { id };
  }
  return { id };
}
