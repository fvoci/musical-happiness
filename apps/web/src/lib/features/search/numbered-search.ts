/**
 * The search with a card's number: `#12` or `12`, asked where a board is
 * shown and for cards (all kinds, or cards alone), finds that board's card
 * numbered 12 (`References.numbered`) and puts it first, before what the
 * words find, which does not list it again. Elsewhere, or for another text,
 * what the words find alone. A number no card of the board has, or one the
 * reader may not see, adds nothing; so does a number that cannot be asked
 * for: the words still answer.
 */
import { cardNumberOf } from '$lib/logic/card-numbers.ts';
import type { SearchAsk, SearchHit } from '$lib/state/content-search.ts';
import type { LinkedBlock } from '$lib/state/references.ts';

export interface NumberedSearch {
  /** The search of the words (`ContentSearch.find`). */
  find: (ask: SearchAsk) => Promise<SearchHit[]>;
  /** The card of the board with the number, if the reader may see it. */
  numbered: (boardId: string, number: number) => Promise<LinkedBlock | undefined>;
  /** The board shown, as the search is asked; none on any other page. */
  boardShown: () => string | undefined;
}

/** What the search finds, with the card of the board shown that the words number. */
export async function findNumbered(search: NumberedSearch, ask: SearchAsk): Promise<SearchHit[]> {
  const number = cardNumberOf(ask.query);
  const boardId = search.boardShown();
  if (number === undefined || boardId === undefined || ask.kind === 'page') {
    return search.find(ask);
  }
  const [hits, card] = await Promise.all([
    search.find(ask),
    search.numbered(boardId, number).catch(() => undefined),
  ]);
  if (!card) return hits;
  const first: SearchHit = {
    id: card.id,
    kind: 'card',
    containerId: card.containerId,
    containerKind: 'board',
    title: card.title,
    number,
    snippet: [],
  };
  const rest = hits.filter((hit) => hit.id !== card.id);
  return [first, ...rest].slice(0, ask.limit ?? rest.length + 1);
}
