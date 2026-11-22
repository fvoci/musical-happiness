/**
 * Going to a card or a page from outside its board or wiki: from the search,
 * from a notification. A card opens over the view of its board that is
 * shown, or else over the board's first view; a page at its wiki's address.
 */
import type { Route, Router } from './router.svelte.ts';

/**
 * The view segment of the address of a card on a board not shown: it names
 * no view, so the board page puts its first view in its place and keeps the
 * card (`BoardPage.svelte`).
 */
export const ANY_VIEW = '-';

/** A card or a page, and where it is. */
export interface Place {
  kind: 'card' | 'page';
  id: string;
  /** The board the card is on, or the wiki the page is in. */
  containerId: string;
  workspaceId: string | undefined;
}

/** The route of a card (over the view shown where its board is shown, else over any) or a page. */
export function routeOfPlace(router: Router, place: Place): Partial<Route> {
  const { workspaceId, containerId, id } = place;
  if (place.kind === 'page') return { workspaceId, wikiId: containerId, pageId: id };
  const { boardId, viewId } = router.route;
  const here = boardId === containerId && viewId !== undefined && viewId !== ANY_VIEW;
  return { workspaceId, boardId: containerId, viewId: here ? viewId : ANY_VIEW, cardId: id };
}

/**
 * Goes to a card or a page. A card of the board shown opens over its view, in
 * place of a card open there; a card of another board opens over that board,
 * as a board chosen in the sidebar. A page of the wiki shown is opened as one
 * of its tree; a page of another wiki over that wiki, as a wiki chosen in
 * the sidebar.
 */
export function goToPlace(router: Router, place: Place): void {
  const route = routeOfPlace(router, place);
  if (place.kind === 'page' && router.route.wikiId === place.containerId) {
    router.openPage(place.id);
  } else if (place.kind === 'card' && route.viewId !== ANY_VIEW) {
    router.openCard(place.id);
  } else {
    const kind = place.kind === 'page' ? 'wiki' : 'board';
    router.openContainer({ id: place.containerId, workspaceId: place.workspaceId ?? '', kind });
    router.go(route, { replace: true });
  }
}
