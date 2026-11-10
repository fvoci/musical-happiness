/** What the face of a card shows of its properties. */
import type { Card, PropertyTemplate } from '$lib/logic/wire.ts';

/** The kinds whose value comes from the card itself, or that say something even when empty. */
const ALWAYS_SHOWN: readonly string[] = [
  'checkbox',
  'createdBy',
  'createdTime',
  'updatedBy',
  'updatedTime',
];

/** Whether a property has something to show on a card: an empty value takes no line. */
export function hasFaceValue(card: Card, template: PropertyTemplate): boolean {
  if (ALWAYS_SHOWN.includes(template.type)) return true;
  const value = card.fields.properties[template.id];
  if (value === undefined || value === '') return false;
  return !Array.isArray(value) || value.length > 0;
}

/**
 * The properties a card shows on its face: those the view turned on, in the
 * order the view lists them, less those the card has no value for. An
 * identifier that is no property of the board (a deleted one, the title) is
 * skipped.
 */
export function faceProperties(
  card: Card,
  templates: readonly PropertyTemplate[],
  visiblePropertyIds: readonly string[],
): PropertyTemplate[] {
  return visiblePropertyIds.flatMap((id) => {
    const template = templates.find((candidate) => candidate.id === id);
    return template && hasFaceValue(card, template) ? [template] : [];
  });
}
