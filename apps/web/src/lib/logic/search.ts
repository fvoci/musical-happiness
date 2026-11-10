/** The search box of a view: which cards mention the text. */
import { foldCase } from './text.ts';
import { cardProperties, type Card, type PropertyTemplate } from './wire.ts';

/**
 * The cards whose title or one of whose property values contains the text,
 * without regard to case (see `foldCase`). A select or multi-select value is
 * searched by the labels of its options. Date values are not searched.
 */
export function searchFilterCards(
  cards: readonly Card[],
  templates: readonly PropertyTemplate[],
  searchText: string,
): Card[] {
  const needle = foldCase(searchText);
  if (!needle) return cards.slice();
  const mentions = (text: string | undefined): boolean =>
    text !== undefined && foldCase(text).includes(needle);

  return cards.filter((card) => {
    if (mentions(card.title)) return true;

    for (const [propertyId, value] of Object.entries(cardProperties(card))) {
      const template = templates.find((candidate) => candidate.id === propertyId);
      // A stored value can be null; the type only says what the editors write.
      if (!template || !(value as typeof value | null) || template.type === 'date') continue;
      const items = Array.isArray(value) ? value : [value];
      const isOption = template.type === 'select' || template.type === 'multiSelect';
      const texts = isOption
        ? items.map((id) => template.options.find((option) => option.id === id)?.value)
        : items;
      if (texts.some(mentions)) return true;
    }
    return false;
  });
}
