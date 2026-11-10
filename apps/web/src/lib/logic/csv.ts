/** The rows of a view as CSV. */
import type { DisplayContext } from './format.ts';
import { encodeCsvText, propertyBehavior } from './properties.ts';
import { cardProperty, type Card, type PropertyTemplate, type ViewQuery } from './wire.ts';

/**
 * A header row and one row per card: the title, then the visible properties
 * in the order of the board. A calendar view also exports the property it
 * places its cards by. `nameHeader` is the translated heading of the title
 * column. Every text cell is quoted, the headings included.
 */
export function tableRows(
  cards: readonly Card[],
  templates: readonly PropertyTemplate[],
  view: Pick<ViewQuery, 'viewType' | 'visiblePropertyIds' | 'dateDisplayPropertyId'>,
  nameHeader: string,
  context: DisplayContext,
): string[][] {
  const visible = templates.filter((template) => view.visiblePropertyIds.includes(template.id));
  if (
    view.viewType === 'calendar' &&
    view.dateDisplayPropertyId &&
    !view.visiblePropertyIds.includes(view.dateDisplayPropertyId)
  ) {
    const dateDisplay = templates.find((template) => template.id === view.dateDisplayPropertyId);
    if (dateDisplay) visible.push(dateDisplay);
  }

  const quoted = (text: string): string => `"${encodeCsvText(text)}"`;
  const rows: string[][] = [
    [quoted(nameHeader), ...visible.map((template) => quoted(template.name))],
  ];
  for (const card of cards) {
    const row = [quoted(card.title)];
    for (const template of visible) {
      let value = cardProperty(card, template.id);
      if (template.type === 'createdBy') value = card.createdBy;
      if (template.type === 'updatedBy') value = card.modifiedBy;
      row.push(propertyBehavior(template.type).exportValue(value, card, template, context));
    }
    rows.push(row);
  }
  return rows;
}

/** The rows as the text of a CSV file. */
export function csvText(rows: readonly (readonly string[])[]): string {
  return rows.map((row) => `${row.join(',')}\r\n`).join('');
}
