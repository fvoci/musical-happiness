/**
 * The address bar: which workspace, board, view and card are shown.
 *
 * The addresses are `/w/<workspace>/boards/<board>/<view>/<card>`, each part
 * optional from the right (`/w/<workspace>` alone is the workspace), under
 * the base path the server serves the app at. The root `/` names no
 * workspace: the app chooses one and puts its address in place. The base
 * path is not built in: the server writes it into index.html (the
 * `fvoci:base` meta element), and it is read from there when the app starts.
 *
 * Every address the app writes is made here (`routePath`, `Router.href`).
 *
 * A card is an address of its own, so opening one adds an entry to the
 * history and "back" closes it. Sheets and the drawer also close on "back",
 * with entries of their own that carry no address (lib/layout/back.svelte.ts);
 * those are passed over here.
 */
import { afterOwnSteps, releaseOverlayEntries } from '$lib/layout/back.svelte.ts';
import { buildUrl, normalizeBasePath, type BoardPlace } from '$lib/logic/urls.ts';

export interface Route {
  workspaceId: string | undefined;
  boardId: string | undefined;
  viewId: string | undefined;
  cardId: string | undefined;
  /** The address is not one of the app's. */
  unknown: boolean;
}

const NOWHERE: Route = {
  workspaceId: undefined,
  boardId: undefined,
  viewId: undefined,
  cardId: undefined,
  unknown: false,
};

/** The base path the server filled into index.html; empty at the root of the site. */
export function readBasePath(document: Document): string {
  const raw = document.querySelector('meta[name="fvoci:base"]')?.getAttribute('content') ?? '';
  // Served as a plain file (the development server), the placeholder is still there.
  return raw.startsWith('{{') ? '' : normalizeBasePath(raw);
}

/** The route an address stands for. `pathname` is the whole path, base path included. */
export function parseRoute(pathname: string, basePath: string): Route {
  const prefix = basePath === '' ? '' : `/${basePath}`;
  if (prefix !== '' && pathname !== prefix && !pathname.startsWith(`${prefix}/`)) {
    return { ...NOWHERE, unknown: true };
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
  if (segments.length === 0) return NOWHERE;
  const [w, workspaceId, boards, boardId, viewId, cardId, ...rest] = segments;
  if (
    w !== 'w' ||
    workspaceId === undefined ||
    (boards !== undefined && boards !== 'boards') ||
    rest.length > 0
  ) {
    return { ...NOWHERE, unknown: true };
  }
  return { workspaceId, boardId, viewId, cardId, unknown: false };
}

/**
 * The route as far as it goes: each part only under the one before it
 * (a card only on a view, a view only on a board, a board only in a workspace).
 */
function settled(route: Partial<Route>): Route {
  const workspaceId = route.workspaceId || undefined;
  const boardId = (workspaceId && route.boardId) || undefined;
  const viewId = (boardId && route.viewId) || undefined;
  const cardId = (viewId && route.cardId) || undefined;
  return { workspaceId, boardId, viewId, cardId, unknown: false };
}

/** The address of a route, from the root of the site. */
export function routePath(route: Partial<Route>, basePath: string): string {
  const { workspaceId, boardId, viewId, cardId } = settled(route);
  if (!workspaceId) return buildUrl('/', basePath);
  const named = [boardId, viewId, cardId].filter((part) => part !== undefined);
  const path = [
    'w',
    encodeURIComponent(workspaceId),
    ...(named.length > 0 ? ['boards'] : []),
    ...named.map((part) => encodeURIComponent(part)),
  ];
  return buildUrl(`/${path.join('/')}`, basePath);
}

export class Router {
  readonly basePath: string;
  route = $state.raw<Route>(NOWHERE);
  /** Whether the card on screen was opened from the board in this visit: then "back" closes it. */
  #cardEntered = false;
  /** Addresses that `go` has yet to write to the history, and the last of them. */
  #writes = 0;
  #heading: string | undefined;

  constructor(basePath: string) {
    this.basePath = basePath;
    this.route = parseRoute(location.pathname, basePath);
  }

  /** Starts following the back and forward buttons. Returns the function that stops. */
  listen(): () => void {
    const onPopState = (): void => {
      // The address of where the reader went is not written yet (it waits for a
      // sheet's step back, which is what arrives here): the bar still shows the old one.
      if (this.#writes > 0) return;
      // An entry of a sheet or the drawer has the address it was opened on: no change.
      const next = parseRoute(location.pathname, this.basePath);
      const now = this.route;
      if (
        next.workspaceId === now.workspaceId &&
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

  /**
   * Loads the page anew at a route (signing out, another account signing
   * in). What is open over the page lets go of its entries first: closing as
   * the page goes (the shell gives way to the sign-in page) would otherwise
   * step back in the history, over the new address, to the one left.
   */
  restart(route: Partial<Route>): void {
    releaseOverlayEntries();
    window.location.assign(this.href(route));
  }

  /** Goes to a route. `replace` puts it in place of the current entry instead of after it. */
  go(route: Partial<Route>, options: { replace?: boolean } = {}): void {
    const next = settled(route);
    const path = routePath(next, this.basePath);
    // While an address waits to be written, that one is where the bar is heading.
    const shown = this.#writes > 0 ? this.#heading : location.pathname;
    if (path !== shown) {
      // The address is written once no sheet that just closed is still giving
      // its entry back (back.svelte.ts): at once, unless one is.
      this.#writes += 1;
      this.#heading = path;
      afterOwnSteps(() => {
        this.#writes -= 1;
        // Going somewhere from inside a drawer or a sheet: the new address takes
        // the place of the overlay's entry, so that "back" returns to where the
        // reader was before opening it.
        const overOverlay = releaseOverlayEntries() > 0;
        if (options.replace || overOverlay) history.replaceState(null, '', path);
        else history.pushState(null, '', path);
      });
    }
    this.route = next;
  }

  /** Shows a board, at the address of its own workspace; its first view is chosen by the page. */
  openBoard(board: BoardPlace): void {
    this.#cardEntered = false;
    this.go({ workspaceId: board.workspaceId, boardId: board.id });
  }

  openView(viewId: string): void {
    this.#cardEntered = false;
    const { workspaceId, boardId } = this.route;
    this.go({ workspaceId, boardId, viewId });
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
      // After whatever still waits to be written: the step is counted from there.
      afterOwnSteps(() => {
        history.back();
      });
    } else {
      this.go({ ...this.route, cardId: undefined }, { replace: true });
    }
  }
}
