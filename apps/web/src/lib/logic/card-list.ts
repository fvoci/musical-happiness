/** From the cards of a board to the cards a view shows, in its order. */
import { applyFilterGroup } from './filter.ts';
import { searchFilterCards } from './search.ts';
import { sortCards, type SortContext } from './sort.ts';
import type { Card, PropertyTemplate, ViewQuery } from './wire.ts';

/**
 * The cards of a view: those the server does not withhold, that pass the
 * view's filter and the search text, sorted the way the view asks.
 */
export function cardsOfView(
  cards: readonly Card[],
  templates: readonly PropertyTemplate[],
  view: ViewQuery,
  searchText: string,
  context: Pick<SortContext, 'lastCommentAt' | 'usernameOf' | 'locale' | 'timeZone'>,
): Card[] {
  let result = cards.filter((card) => !card.limited);
  result = applyFilterGroup(view.filter, templates, result, context);
  if (searchText) result = searchFilterCards(result, templates, searchText);
  return sortCards(result, {
    ...context,
    templates,
    sortOptions: view.sortOptions,
    cardOrder: view.cardOrder,
  });
}

/** How many cards of a board the server withholds. */
export function hiddenCardCount(cards: readonly Card[]): number {
  return cards.filter((card) => card.limited).length;
}
