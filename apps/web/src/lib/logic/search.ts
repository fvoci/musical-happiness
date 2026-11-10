/** The search box of a view: which cards mention the text. */
import { cardProperties, type Card, type PropertyTemplate } from './wire.ts';

/**
 * The cards whose title or one of whose property values contains the text,
 * ignoring case. A select value is searched by the label of its option.
 * Date values are not searched.
 *
 * QUIRK(Q3): a multi-select value only matches when the text equals a whole
 * option label, while every other kind matches on a part. Kept as in the
 * original.
 */
export function searchFilterCards(
  cards: readonly Card[],
  templates: readonly PropertyTemplate[],
  searchText: string,
): Card[] {
  const needle = searchText.toLocaleLowerCase();
  if (!needle) return cards.slice();

  return cards.filter((card) => {
    if (card.title.toLocaleLowerCase().includes(needle)) return true;

    for (const [propertyId, value] of Object.entries(cardProperties(card))) {
      const template = templates.find((candidate) => candidate.id === propertyId);
      // A stored value can be null; the type only says what the editors write.
      if (!template || !(value as typeof value | null)) continue;
      const label = (optionId: string): string | undefined =>
        template.options.find((option) => option.id === optionId)?.value.toLowerCase();

      if (template.type === 'select') {
        if (typeof value === 'string' && label(value)?.includes(needle)) return true;
      } else if (template.type === 'multiSelect') {
        const labels = (Array.isArray(value) ? value : [value]).map(label);
        if (labels.includes(needle)) return true;
      } else if (template.type !== 'date' && value.toString().toLowerCase().includes(needle)) {
        return true;
      }
    }
    return false;
  });
}
