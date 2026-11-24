/**
 * The address bar: which workspace, board, view and card are shown.
 *
 * Pure route parsing and formatting live in `route-path.ts`; this module holds
 * the reactive `Router` synced with `history` and `popstate`.
 */
import { afterOwnSteps, releaseOverlayEntries } from '$lib/layout/back.svelte.ts';
import { buildUrl, type ContainerPlace } from '$lib/logic/urls.ts';

import {
  ANY_VIEW,
  NOWHERE,
  parseRoute,
  placeRoute,
  readBasePath,
  routePath,
  settled,
  shownContainerId,
  type CourseTab,
  type Route,
} from './route-path.ts';

export {
  ANY_VIEW,
  parseRoute,
  placeRoute,
  readBasePath,
  routePath,
  shownContainerId,
  type CourseTab,
  type Route,
};

function sameRoute(a: Route, b: Route): boolean {
  return (
    a.passwordReset === b.passwordReset &&
    a.passwordForgot === b.passwordForgot &&
    a.emailVerify === b.emailVerify &&
    a.emailConfirm === b.emailConfirm &&
    a.emailRevert === b.emailRevert &&
    a.orgId === b.orgId &&
    a.meTime === b.meTime &&
    a.workspaceId === b.workspaceId &&
    a.boardId === b.boardId &&
    a.viewId === b.viewId &&
    a.cardId === b.cardId &&
    a.wikiId === b.wikiId &&
    a.pageId === b.pageId &&
    a.diaryId === b.diaryId &&
    a.monthDay === b.monthDay &&
    a.year === b.year &&
    a.courseId === b.courseId &&
    a.unitId === b.unitId &&
    a.courseTab === b.courseTab &&
    a.meetingId === b.meetingId &&
    a.learningToday === b.learningToday &&
    a.invitation === b.invitation &&
    a.readToken === b.readToken &&
    a.shared === b.shared &&
    a.unknown === b.unknown
  );
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
      if (this.#writes > 0) return;
      const next = parseRoute(location.pathname, this.basePath, location.search);
      const now = this.route;
      if (sameRoute(next, now)) return;
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

  /** Loads the page anew at a route (signing out, another account signing in). */
  restart(route: Partial<Route>, options: { replace?: boolean; query?: string } = {}): void {
    const overOverlay = releaseOverlayEntries() > 0;
    const address = this.href(route) + (options.query ? `?${options.query}` : '');
    if (overOverlay || options.replace) window.location.replace(address);
    else window.location.assign(address);
  }

  /** Goes to a route. `replace` puts it in place of the current entry instead of after it. */
  go(route: Partial<Route>, options: { replace?: boolean } = {}): void {
    const next = settled(route);
    const path = routePath(next, this.basePath);
    if (Boolean(next.shared) !== Boolean(this.route.shared)) {
      window.location.assign(path);
      return;
    }
    const here =
      next.readToken || next.year || this.route.year || next.courseTab || this.route.courseTab
        ? `${location.pathname}${location.search}`
        : location.pathname;
    const shown = this.#writes > 0 ? this.#heading : here;
    if (path !== shown) {
      this.#writes += 1;
      this.#heading = path;
      afterOwnSteps(() => {
        this.#writes -= 1;
        const overOverlay = releaseOverlayEntries() > 0;
        if (options.replace || overOverlay) history.replaceState(null, '', path);
        else history.pushState(null, '', path);
      });
    }
    this.route = next;
  }

  openContainer(container: ContainerPlace): void {
    this.#cardEntered = false;
    this.go(placeRoute(container));
  }

  openPage(pageId: string | undefined, options: { replace?: boolean } = {}): void {
    const { workspaceId, wikiId } = this.route;
    if (wikiId) this.go({ workspaceId, wikiId, pageId }, options);
  }

  openDay(monthDay: string, options: { replace?: boolean } = {}): void {
    const { workspaceId, diaryId } = this.route;
    if (diaryId) this.go({ workspaceId, diaryId, monthDay }, options);
  }

  openUnit(unitId: string | undefined, options: { replace?: boolean } = {}): void {
    const { workspaceId, courseId } = this.route;
    if (courseId) this.go({ workspaceId, courseId, unitId }, options);
  }

  /** Switches the active tab of the course shown (`units` clears `?tab=`). */
  openCourseTab(tab: CourseTab, options: { meetingId?: string; replace?: boolean } = {}): void {
    const { workspaceId, courseId, unitId } = this.route;
    if (!courseId) return;
    this.go(
      {
        workspaceId,
        courseId,
        unitId,
        ...(tab === 'units' ? {} : { courseTab: tab }),
        ...(tab === 'meetings' && options.meetingId ? { meetingId: options.meetingId } : {}),
      },
      options,
    );
  }

  openView(viewId: string): void {
    this.#cardEntered = false;
    const { workspaceId, boardId, readToken } = this.route;
    this.go({ workspaceId, boardId, viewId, readToken });
  }

  openCard(cardId: string): void {
    const replacing = this.route.cardId !== undefined;
    this.go({ ...this.route, cardId }, { replace: replacing });
    if (!replacing) this.#cardEntered = true;
  }

  closeCard(): void {
    if (this.route.cardId === undefined) return;
    if (this.#cardEntered) {
      this.#cardEntered = false;
      afterOwnSteps(() => {
        history.back();
      });
    } else {
      this.go({ ...this.route, cardId: undefined }, { replace: true });
    }
  }
}
