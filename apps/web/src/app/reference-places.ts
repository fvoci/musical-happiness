/**
 * Where a reference in a body leads, by the address bar: a card of the board
 * that is shown opens over the view that is shown, a card of another board
 * on that board's first view, and a page in its wiki. A reader by a link
 * reads one board, and nothing else has an address there.
 */
import { referenceAt } from '$lib/logic/reference-address.ts';
import type { LinkedBlock, ReferencePlaces } from '$lib/state/index.ts';

import { ANY_VIEW, type Route, type Router } from './router.svelte.ts';

/** The route that shows what is named; none for what has no screen. */
function routeOf(router: Router, linked: LinkedBlock): Partial<Route> | undefined {
  const { route } = router;
  if (linked.kind === 'card' && linked.containerId === route.boardId) {
    return { ...route, cardId: linked.id };
  }
  if (route.shared) return undefined;
  const { workspaceId } = route;
  if (linked.kind === 'page') return { workspaceId, wikiId: linked.containerId, pageId: linked.id };
  return { workspaceId, boardId: linked.containerId, viewId: ANY_VIEW, cardId: linked.id };
}

export function referencePlaces(router: Router): ReferencePlaces {
  return {
    href: (linked) => {
      const route = routeOf(router, linked);
      return route ? router.href(route) : undefined;
    },
    open: (linked) => {
      const route = routeOf(router, linked);
      if (!route) return;
      if (linked.kind === 'card' && linked.containerId === router.route.boardId) {
        router.openCard(linked.id);
      } else if (linked.kind === 'page' && linked.containerId === router.route.wikiId) {
        router.openPage(linked.id);
      } else router.go(route);
    },
    named: (address) =>
      referenceAt(address, {
        origin: location.origin,
        basePath: router.basePath,
        workspaceId: router.route.workspaceId,
      }),
  };
}
