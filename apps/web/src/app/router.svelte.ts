/**
 * The address bar: which board, view and card are shown.
 *
 * The addresses are `/board/<board>/<view>/<card>`, each part optional from
 * the right, under the base path the server serves the app at. The base path
 * is not built in: the server writes it into index.html (the `fvoci:base`
 * meta element), and it is read from there when the app starts.
 *
 * A card is an address of its own, so opening one adds an entry to the
 * history and "back" closes it. Sheets and the drawer also close on "back",
 * with entries of their own that carry no address (lib/layout/back.svelte.ts);
 * those are passed over here.
 */
import { releaseOverlayEntries } from '$lib/layout/back.svelte.ts';
import { buildUrl, fillPath, normalizeBasePath } from '$lib/logic/urls.ts';

export interface Route {
  boardId: string | undefined;
  viewId: string | undefined;
  cardId: string | undefined;
  /** The address is not one of the app's. */
  unknown: boolean;
}

const BOARD_PATTERN = '/board/:boardId?/:viewId?/:cardId?';

/** The base path the server filled into index.html; empty at the root of the site. */
export function readBasePath(document: Document): string {
  const raw = document.querySelector('meta[name="fvoci:base"]')?.getAttribute('content') ?? '';
  // Served as a plain file (the development server), the placeholder is still there.
  return raw.startsWith('{{') ? '' : normalizeBasePath(raw);
}

/** The route an address stands for. `pathname` is the whole path, base path included. */
export function parseRoute(pathname: string, basePath: string): Route {
  const none: Route = { boardId: undefined, viewId: undefined, cardId: undefined, unknown: false };
  const prefix = basePath === '' ? '' : `/${basePath}`;
  if (prefix !== '' && pathname !== prefix && !pathname.startsWith(`${prefix}/`)) {
    return { ...none, unknown: true };
  }
  const segments = pathname
    .slice(prefix.length)
    .split('/')
    .filter((segment) => segment !== '')
    .map((segment) => {
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    });
  if (segments.length === 0) return none;
  const [first, boardId, viewId, cardId, ...rest] = segments;
  if (first !== 'board' || rest.length > 0) return { ...none, unknown: true };
  return { boardId, viewId, cardId, unknown: false };
}

/** The address of a route, from the root of the site. */
export function routePath(route: Partial<Route>, basePath: string): string {
  if (!route.boardId) return buildUrl('/', basePath);
  const encoded = {
    boardId: encodeURIComponent(route.boardId),
    viewId: route.viewId && encodeURIComponent(route.viewId),
    cardId: route.viewId && route.cardId && encodeURIComponent(route.cardId),
  };
  return buildUrl(fillPath(BOARD_PATTERN, encoded), basePath);
}

export class Router {
  readonly basePath: string;
  route = $state.raw<Route>({
    boardId: undefined,
    viewId: undefined,
    cardId: undefined,
    unknown: false,
  });
  /** Whether the card on screen was opened from the board in this visit: then "back" closes it. */
  #cardEntered = false;

  constructor(basePath: string) {
    this.basePath = basePath;
    this.route = parseRoute(location.pathname, basePath);
  }

  /** Starts following the back and forward buttons. Returns the function that stops. */
  listen(): () => void {
    const onPopState = (): void => {
      // An entry of a sheet or the drawer has the address it was opened on: no change.
      const next = parseRoute(location.pathname, this.basePath);
      const now = this.route;
      if (
        next.boardId === now.boardId &&
        next.viewId === now.viewId &&
        next.cardId === now.cardId &&
        next.unknown === now.unknown
      ) {
        return;
      }
      if (next.cardId !== now.cardId) this.#cardEntered = false;
      this.route = next;
    };
    window.addEventListener('popstate', onPopState);
    return () => {
      window.removeEventListener('popstate', onPopState);
    };
  }

  href(route: Partial<Route>): string {
    return routePath(route, this.basePath);
  }

  /** Goes to a route. `replace` puts it in place of the current entry instead of after it. */
  go(route: Partial<Route>, options: { replace?: boolean } = {}): void {
    const next: Route = {
      boardId: route.boardId,
      viewId: route.boardId ? route.viewId : undefined,
      cardId: route.boardId && route.viewId ? route.cardId : undefined,
      unknown: false,
    };
    const path = routePath(next, this.basePath);
    if (path !== location.pathname) {
      // Going somewhere from inside a drawer or a sheet: the new address takes
      // the place of the overlay's entry, so that "back" returns to where the
      // reader was before opening it.
      const overOverlay = releaseOverlayEntries() > 0;
      if (options.replace || overOverlay) history.replaceState(null, '', path);
      else history.pushState(null, '', path);
    }
    this.route = next;
  }

  /** Shows a board; its first view is chosen by the page. */
  openBoard(boardId: string): void {
    this.#cardEntered = false;
    this.go({ boardId });
  }

  openView(viewId: string): void {
    this.#cardEntered = false;
    this.go({ boardId: this.route.boardId, viewId });
  }

  /** Opens a card over the view that is shown. */
  openCard(cardId: string): void {
    const replacing = this.route.cardId !== undefined;
    this.go({ ...this.route, cardId }, { replace: replacing });
    if (!replacing) this.#cardEntered = true;
  }

  /**
   * Closes the card. When it was opened from the board, that is one step
   * back in the history, so that "forward" opens it again; when the visit
   * started on the card's address, there is nothing to go back to and the
   * address is replaced.
   */
  closeCard(): void {
    if (this.route.cardId === undefined) return;
    if (this.#cardEntered) {
      this.#cardEntered = false;
      history.back();
    } else {
      this.go({ ...this.route, cardId: undefined }, { replace: true });
    }
  }
}
