/**
 * The address bar: which workspace, board, view and card are shown.
 *
 * The addresses are `/w/<workspace>/boards/<board>/<view>/<card>`,
 * `/w/<workspace>/wikis/<wiki>/<page>` and `/w/<workspace>/diaries/<diary>/<MM-DD>`
 * (a day of the year, the page of a diary), each part optional from the right
 * (`/w/<workspace>` alone is the workspace), and
 * `/org/<organization>`, the screen of its administrators, under
 * the base path the server serves the app at. The link of an invitation is an
 * address of its own, `/invite/<token>`, which the app reads and never writes. A board's public link is
 * `/shared/<board>/<view>/<card>?r=<read token>`, the view and the card
 * optional: whoever has it reads the board, signed in or not. The root `/` names no
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
import { buildUrl, normalizeBasePath, type ContainerPlace } from '$lib/logic/urls.ts';

export interface Route {
  /** The standalone password reset page; its token is in the URL fragment. */
  passwordReset?: true;
  passwordForgot?: true;
  /** The page the link that confirms a registration's address opens; its token is in the fragment. */
  emailVerify?: true;
  /** The page the link that changes an account's address opens; its token is in the fragment. */
  emailConfirm?: true;
  /** The organization whose screen is shown; then nothing else is. */
  orgId?: string | undefined;
  workspaceId: string | undefined;
  boardId: string | undefined;
  viewId: string | undefined;
  cardId: string | undefined;
  /** A wiki of the workspace, in place of a board, and the page of it shown. */
  wikiId?: string | undefined;
  pageId?: string | undefined;
  /** A diary of the workspace, in place of a board, and the day of the year it is open on (`MM-DD`). */
  diaryId?: string | undefined;
  monthDay?: string | undefined;
  /**
   * The token of an invitation: the address is the link of one
   * (`/invite/<token>`), and names nothing else.
   */
  invitation?: string | undefined;
  /** The read token of a public link: the board is read by it alone, with no workspace. */
  readToken?: string;
  /**
   * The address is under `/shared`, a public link's, whether it is a whole one
   * or not: the reader's screen shows it, never the app.
   */
  shared?: true;
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

/**
 * The route an address stands for. `pathname` is the whole path, base path
 * included; `search` the query, where a public link has its read token.
 */
export function parseRoute(pathname: string, basePath: string, search = ''): Route {
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
  if (segments[0] === 'forgot-password' && segments.length === 1) {
    return { ...NOWHERE, passwordForgot: true };
  }
  if (segments[0] === 'reset-password' && segments.length === 1) {
    return { ...NOWHERE, passwordReset: true };
  }
  if (segments[0] === 'verify-email' && segments.length === 1) {
    return { ...NOWHERE, emailVerify: true };
  }
  if (segments[0] === 'confirm-email' && segments.length === 1) {
    return { ...NOWHERE, emailConfirm: true };
  }
  if (segments[0] === 'invite' && segments.length === 2) {
    return { ...NOWHERE, invitation: segments[1] };
  }
  if (segments[0] === 'shared') return sharedRoute(segments.slice(1), search);
  if (segments[0] === 'org') {
    return segments.length === 2
      ? { ...NOWHERE, orgId: segments[1] }
      : { ...NOWHERE, unknown: true };
  }
  const [w, workspaceId, section, boardId, viewId, cardId, ...rest] = segments;
  if (section === 'wikis' && boardId !== undefined && cardId === undefined && w === 'w') {
    return { ...NOWHERE, workspaceId, wikiId: boardId, pageId: viewId };
  }
  if (section === 'diaries' && boardId !== undefined && cardId === undefined && w === 'w') {
    return { ...NOWHERE, workspaceId, diaryId: boardId, monthDay: viewId };
  }
  if (
    w !== 'w' ||
    workspaceId === undefined ||
    (section !== undefined && section !== 'boards') ||
    rest.length > 0
  ) {
    return { ...NOWHERE, unknown: true };
  }
  return { workspaceId, boardId, viewId, cardId, unknown: false };
}

/** The route of a public link: a board, then a view and a card, and the read token. */
function sharedRoute(segments: string[], search: string): Route {
  const [boardId, viewId, cardId, ...rest] = segments;
  const readToken = new URLSearchParams(search).get('r');
  if (boardId === undefined || readToken === null || readToken === '' || rest.length > 0) {
    return { ...NOWHERE, unknown: true, shared: true };
  }
  return {
    workspaceId: undefined,
    boardId,
    viewId,
    cardId,
    readToken,
    unknown: false,
    shared: true,
  };
}

/**
 * In the place of a view in an address, where the view is not known (a card
 * of a board that is not open): the page shows the board's first view, as
 * for any view that is not there, and keeps the card.
 */
export const ANY_VIEW = '-';

/**
 * The route as far as it goes: each part only under the one before it
 * (a card only on a view, a view only on a board, a board only in a workspace),
 * and an organization's screen alone.
 */
function settled(route: Partial<Route>): Route {
  if (route.orgId) return { ...NOWHERE, orgId: route.orgId };
  const readToken = route.readToken || undefined;
  const workspaceId = (!readToken && route.workspaceId) || undefined;
  // A wiki is the workspace's alone (no public link reads one yet: W4), and no board is with it.
  const wikiId = (workspaceId && route.wikiId) || undefined;
  if (wikiId) return { ...NOWHERE, workspaceId, wikiId, pageId: route.pageId || undefined };
  const diaryId = (workspaceId && route.diaryId) || undefined;
  if (diaryId) return { ...NOWHERE, workspaceId, diaryId, monthDay: route.monthDay || undefined };
  const boardId = ((workspaceId || readToken) && route.boardId) || undefined;
  const viewId = (boardId && route.viewId) || undefined;
  const cardId = (viewId && route.cardId) || undefined;
  if (readToken && boardId)
    return { workspaceId, boardId, viewId, cardId, readToken, unknown: false, shared: true };
  return { workspaceId, boardId, viewId, cardId, unknown: false };
}

/**
 * The route of a board, or of a wiki or a diary where it is one (`kind`). A
 * diary's names no day: its page opens on today's.
 */
export function placeRoute(container: ContainerPlace): Partial<Route> {
  const { workspaceId, id } = container;
  if (container.kind === 'wiki') return { workspaceId, wikiId: id };
  if (container.kind === 'diary') return { workspaceId, diaryId: id };
  return { workspaceId, boardId: id };
}

/** The board, wiki or diary a route shows. */
export function shownContainerId(route: Route): string | undefined {
  return route.boardId ?? route.wikiId ?? route.diaryId;
}

/** The address of a route, from the root of the site. */
export function routePath(route: Partial<Route>, basePath: string): string {
  if (route.orgId) return buildUrl(`/org/${encodeURIComponent(route.orgId)}`, basePath);
  const { workspaceId, boardId, viewId, cardId, readToken, wikiId, pageId, diaryId, monthDay } =
    settled(route);
  if (readToken && boardId) {
    const parts = [boardId, viewId, cardId].filter((part) => part !== undefined);
    const path = ['shared', ...parts.map((part) => encodeURIComponent(part))].join('/');
    return `${buildUrl(`/${path}`, basePath)}?r=${encodeURIComponent(readToken)}`;
  }
  if (!workspaceId) return buildUrl('/', basePath);
  const named = (
    wikiId ? [wikiId, pageId] : diaryId ? [diaryId, monthDay] : [boardId, viewId, cardId]
  ).filter((part) => part !== undefined);
  const section = wikiId ? 'wikis' : diaryId ? 'diaries' : 'boards';
  const path = [
    'w',
    encodeURIComponent(workspaceId),
    ...(named.length > 0 ? [section] : []),
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
    this.route = parseRoute(location.pathname, basePath, location.search);
  }

  /** Starts following the back and forward buttons. Returns the function that stops. */
  listen(): () => void {
    const onPopState = (): void => {
      // The address of where the reader went is not written yet (it waits for a
      // sheet's step back, which is what arrives here): the bar still shows the old one.
      if (this.#writes > 0) return;
      // An entry of a sheet or the drawer has the address it was opened on: no change.
      const next = parseRoute(location.pathname, this.basePath, location.search);
      const now = this.route;
      if (
        next.passwordReset === now.passwordReset &&
        next.passwordForgot === now.passwordForgot &&
        next.emailVerify === now.emailVerify &&
        next.emailConfirm === now.emailConfirm &&
        next.orgId === now.orgId &&
        next.workspaceId === now.workspaceId &&
        next.boardId === now.boardId &&
        next.viewId === now.viewId &&
        next.cardId === now.cardId &&
        next.wikiId === now.wikiId &&
        next.pageId === now.pageId &&
        next.diaryId === now.diaryId &&
        next.monthDay === now.monthDay &&
        next.invitation === now.invitation &&
        next.readToken === now.readToken &&
        next.shared === now.shared &&
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

  /** The address of the page of an invitation, by its token. */
  inviteHref(token: string): string {
    return buildUrl(`/invite/${encodeURIComponent(token)}`, this.basePath);
  }

  /**
   * Loads the page anew at a route (signing out, another account signing
   * in). What is open over the page lets go of its entries first: closing as
   * the page goes (the shell gives way to the sign-in page) would otherwise
   * step back in the history, over the new address, to the one left. The new
   * address takes the place of such an entry, as `go` does it, and of the
   * current one where `replace` asks for it.
   */
  restart(route: Partial<Route>, options: { replace?: boolean; query?: string } = {}): void {
    const overOverlay = releaseOverlayEntries() > 0;
    // What the page that starts is told once (`query`), as a provider tells it what became of a sign-in.
    const address = this.href(route) + (options.query ? `?${options.query}` : '');
    if (overOverlay || options.replace) window.location.replace(address);
    else window.location.assign(address);
  }

  /** Goes to a route. `replace` puts it in place of the current entry instead of after it. */
  go(route: Partial<Route>, options: { replace?: boolean } = {}): void {
    const next = settled(route);
    const path = routePath(next, this.basePath);
    // Between a public link and the app the page starts anew: each is a page of its own.
    if (Boolean(next.shared) !== Boolean(this.route.shared)) {
      window.location.assign(path);
      return;
    }
    // While an address waits to be written, that one is where the bar is heading.
    const here = next.readToken ? `${location.pathname}${location.search}` : location.pathname;
    const shown = this.#writes > 0 ? this.#heading : here;
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

  /**
   * Shows a board, at the address of its own workspace; its first view is
   * chosen by the page. A wiki (`kind`) is shown at a wiki's address, its
   * first page chosen by its page.
   */
  openContainer(container: ContainerPlace): void {
    this.#cardEntered = false;
    this.go(placeRoute(container));
  }

  /** Shows a page of the wiki shown. `replace`: in place of the address there. */
  openPage(pageId: string | undefined, options: { replace?: boolean } = {}): void {
    const { workspaceId, wikiId } = this.route;
    if (wikiId) this.go({ workspaceId, wikiId, pageId }, options);
  }

  /** Shows a day of the year of the diary shown. `replace`: in place of the address there. */
  openDay(monthDay: string, options: { replace?: boolean } = {}): void {
    const { workspaceId, diaryId } = this.route;
    if (diaryId) this.go({ workspaceId, diaryId, monthDay }, options);
  }

  openView(viewId: string): void {
    this.#cardEntered = false;
    const { workspaceId, boardId, readToken } = this.route;
    this.go({ workspaceId, boardId, viewId, readToken });
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
