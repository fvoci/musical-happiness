/** Which conditions a filter offers for each kind of value, and what they are called. */
import type { FilterValueType } from './properties.ts';
import type { FilterCondition } from './wire.ts';

/** The conditions offered for a kind of value; the first is the default. */
const CONDITIONS: Record<
  Exclude<FilterValueType, 'none' | 'person'>,
  readonly FilterCondition[]
> = {
  options: ['includes', 'notIncludes', 'isEmpty', 'isNotEmpty'],
  boolean: ['isSet', 'isNotSet'],
  text: ['is', 'contains', 'notContains', 'startsWith', 'notStartsWith', 'endsWith', 'notEndsWith'],
  date: ['is', 'isBefore', 'isAfter', 'isSet', 'isNotSet'],
};

function conditionsOf(valueType: FilterValueType): readonly FilterCondition[] | undefined {
  // People are chosen from a list like options are.
  if (valueType === 'person') return CONDITIONS.options;
  return valueType === 'none' ? undefined : CONDITIONS[valueType];
}

/**
 * The condition to use after the property of a filter changed: the current
 * one when the new kind of value offers it, otherwise that kind's default.
 *
 * QUIRK(Q13): for a person property the original keeps no condition and
 * falls back to `includes`, although its label function treats people like
 * options. Kept as in the original.
 */
export function filterConditionValidOrDefault(
  valueType: FilterValueType,
  current: FilterCondition,
): FilterCondition {
  if (valueType === 'none' || valueType === 'person') return 'includes';
  const conditions = CONDITIONS[valueType];
  return conditions.includes(current) ? current : (conditions[0] ?? 'includes');
}

/** The translated names of the conditions. */
export type FilterConditionMessages = Record<FilterCondition, () => string>;

/**
 * What a condition is called for a kind of value. A condition that kind does
 * not offer is shown as the kind's default.
 */
export function filterConditionDisplayString(
  condition: FilterCondition,
  messages: FilterConditionMessages,
  valueType: FilterValueType,
): string {
  const conditions = conditionsOf(valueType);
  if (!conditions) return '(unknown)';
  const shown = conditions.includes(condition) ? condition : (conditions[0] ?? condition);
  return messages[shown]();
}
