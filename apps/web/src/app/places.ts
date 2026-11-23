/**
 * Going to a card or a page from outside its board or wiki: from the search,
 * from a notification. A card opens over the view of its board that is
 * shown, or else over the board's first view; a page at its wiki's address.
 */
import { parseCalendarDate } from '$lib/logic/dates.ts';

import type { Route, Router } from './router.svelte.ts';

/**
 * The view segment of the address of a card on a board not shown: it names
 * no view, so the board page puts its first view in its place and keeps the
 * card (`BoardPage.svelte`).
 */
export const ANY_VIEW = '-';

/** A card, a page or a diary entry, and where it is. */
export interface Place {
  kind: 'card' | 'page' | 'entry';
  id: string;
  /** The board the card is on, the wiki the page is in, or the diary the entry is in. */
  containerId: string;
  workspaceId: string | undefined;
  /** The entry's day (`YYYY-MM-DD`), for a diary entry. */
  title?: string;
}

/** The route of a card (over the view shown where its board is shown, else over any), a page, or a diary entry. */
export function routeOfPlace(router: Router, place: Place): Partial<Route> {
  const { workspaceId, containerId, id } = place;
  if (place.kind === 'entry') {
    const date = parseCalendarDate(place.title ?? '');
    return {
      workspaceId,
      diaryId: containerId,
      monthDay: date ? date.slice(5) : (place.title ?? '').slice(5),
      ...(date ? { year: date.slice(0, 4) } : {}),
    };
  }
  if (place.kind === 'page') return { workspaceId, wikiId: containerId, pageId: id };
  const { boardId, viewId } = router.route;
  const here = boardId === containerId && viewId !== undefined && viewId !== ANY_VIEW;
  return { workspaceId, boardId: containerId, viewId: here ? viewId : ANY_VIEW, cardId: id };
}

/**
 * Goes to a card, a page or a diary entry. A card of the board shown opens over its view, in
 * place of a card open there; a card of another board opens over that board,
 * as a board chosen in the sidebar. A page of the wiki shown is opened as one
 * of its tree; a page of another wiki over that wiki, as a wiki chosen in
 * the sidebar. A diary entry opens at its day in its diary.
 */
export function goToPlace(router: Router, place: Place): void {
  const route = routeOfPlace(router, place);
  if (place.kind === 'page' && router.route.wikiId === place.containerId) {
    router.openPage(place.id);
  } else if (place.kind === 'card' && route.viewId !== ANY_VIEW) {
    router.openCard(place.id);
  } else if (place.kind === 'entry' && router.route.diaryId === place.containerId) {
    router.go(route);
  } else {
    const kind = ({ card: 'board', page: 'wiki', entry: 'diary' } as const)[place.kind];
    router.openContainer({ id: place.containerId, workspaceId: place.workspaceId ?? '', kind });
    router.go(route, { replace: true });
  }
}

/**
 * Puts the focus where a card or a page was opened (`goToPlace`): in the card
 * where one is open, else on the page.
 */
export function focusThePlace(): void {
  const card = document.querySelector<HTMLElement>('[data-card-dialog]');
  (card ?? document.querySelector<HTMLElement>('main'))?.focus();
}
