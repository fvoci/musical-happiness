/**
 * Where a reference in a body leads, by the address bar: a card of the board
 * that is shown opens over the view that is shown, a card of another board
 * on that board's first view. A page has no screen yet, and so no address.
 */
import { referenceAt } from '$lib/logic/reference-address.ts';
import type { LinkedBlock, ReferencePlaces } from '$lib/state/index.ts';

import { ANY_VIEW, type Route, type Router } from './router.svelte.ts';

/** The route that shows what is named; none for what has no screen. */
function routeOf(router: Router, linked: LinkedBlock): Partial<Route> | undefined {
  if (linked.kind !== 'card') return undefined;
  const { route } = router;
  if (linked.boardId === route.boardId) return { ...route, cardId: linked.id };
  // A link's reader reads one board: nothing of another has an address there.
  if (route.shared) return undefined;
  const { workspaceId } = route;
  return { workspaceId, boardId: linked.boardId, viewId: ANY_VIEW, cardId: linked.id };
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
      if (linked.boardId === router.route.boardId) router.openCard(linked.id);
      else router.go(route);
    },
    named: (address) =>
      referenceAt(address, {
        origin: location.origin,
        basePath: router.basePath,
        workspaceId: router.route.workspaceId,
      }),
  };
}
