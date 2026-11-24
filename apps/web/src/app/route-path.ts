/**
 * Pure URL route parsing and formatting for the app (`router.svelte.ts`).
 */
import { buildUrl, normalizeBasePath, type ContainerPlace } from '$lib/logic/urls.ts';

export type CourseTab = 'units' | 'meetings' | 'exams';

export interface Route {
  /** The standalone password reset page; its token is in the URL fragment. */
  passwordReset?: true;
  passwordForgot?: true;
  /** The page the link that confirms a registration's address opens; its token is in the fragment. */
  emailVerify?: true;
  /** The page the link that changes an account's address opens; its token is in the fragment. */
  emailConfirm?: true;
  /** The page the link that gives an account its address back opens; its token is in the fragment. */
  emailRevert?: true;
  /** The organization whose screen is shown; then nothing else is. */
  orgId?: string | undefined;
  /** The reader's personal time log (`/me/time`). */
  meTime?: true;
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
  /** An optional year of that day to expand on arrival (`?year=YYYY`). */
  year?: string | undefined;
  /** A course of the workspace, in place of a board, and the unit shown. */
  courseId?: string | undefined;
  unitId?: string | undefined;
  /** Optional non-default course tab (`?tab=meetings` or `?tab=exams`). */
  courseTab?: Exclude<CourseTab, 'units'> | undefined;
  /** Optional meeting to highlight on the Meetings tab (`?tab=meetings&meeting=<id>`). */
  meetingId?: string | undefined;
  /** The workspace's personal "Today's reviews" screen (`/w/<workspace>/learning/reviews/today`). */
  reviewsToday?: true;
  /** The workspace "Today's learning" screen (`/w/<workspace>/learning/today`). */
  learningToday?: true;
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

export const NOWHERE: Route = {
  workspaceId: undefined,
  boardId: undefined,
  viewId: undefined,
  cardId: undefined,
  unknown: false,
};

/**
 * In the place of a view in an address, where the view is not known (a card
 * of a board that is not open): the page shows the board's first view, as
 * for any view that is not there, and keeps the card.
 */
export const ANY_VIEW = '-';

/** A four-digit year (`0001`–`9999`) for a diary page's `?year=YYYY` query; undefined for anything else. */
function diaryYear(raw: string | null | undefined): string | undefined {
  return raw && /^\d{4}$/.test(raw) && raw !== '0000' ? raw : undefined;
}

/** The base path the server filled into index.html; empty at the root of the site. */
export function readBasePath(document: Document): string {
  const raw = document.querySelector('meta[name="fvoci:base"]')?.getAttribute('content') ?? '';
  // Served as a plain file (the development server), the placeholder is still there.
  return raw.startsWith('{{') ? '' : normalizeBasePath(raw);
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
  if (segments[0] === 'revert-email' && segments.length === 1) {
    return { ...NOWHERE, emailRevert: true };
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
  if (segments[0] === 'me' && segments[1] === 'time' && segments.length === 2) {
    return { ...NOWHERE, meTime: true };
  }
  const [w, workspaceId, section, boardId, viewId, cardId, ...rest] = segments;
  if (section === 'wikis' && boardId !== undefined && cardId === undefined && w === 'w') {
    return { ...NOWHERE, workspaceId, wikiId: boardId, pageId: viewId };
  }
  if (section === 'diaries' && boardId !== undefined && cardId === undefined && w === 'w') {
    const rawYear = viewId !== undefined ? new URLSearchParams(search).get('year') : null;
    const year = diaryYear(rawYear);
    return {
      ...NOWHERE,
      workspaceId,
      diaryId: boardId,
      monthDay: viewId,
      ...(year ? { year } : {}),
    };
  }
  if (section === 'courses' && boardId !== undefined && cardId === undefined && w === 'w') {
    const params = new URLSearchParams(search);
    const rawTab = params.get('tab');
    const courseTab = rawTab === 'meetings' || rawTab === 'exams' ? rawTab : undefined;
    const rawMeeting = courseTab === 'meetings' ? params.get('meeting') : null;
    const meetingId = rawMeeting ? rawMeeting : undefined;
    return {
      ...NOWHERE,
      workspaceId,
      courseId: boardId,
      unitId: viewId,
      ...(courseTab ? { courseTab } : {}),
      ...(meetingId ? { meetingId } : {}),
    };
  }
  if (section === 'learning' && boardId === 'today' && viewId === undefined && w === 'w') {
    return { ...NOWHERE, workspaceId, learningToday: true };
  }
  if (
    w === 'w' &&
    workspaceId !== undefined &&
    section === 'learning' &&
    boardId === 'reviews' &&
    viewId === 'today' &&
    cardId === undefined
  ) {
    return { ...NOWHERE, workspaceId, reviewsToday: true };
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

/**
 * The route as far as it goes: each part only under the one before it
 * (a card only on a view, a view only on a board, a board only in a workspace),
 * and an organization's screen alone.
 */
export function settled(route: Partial<Route>): Route {
  if (route.orgId) return { ...NOWHERE, orgId: route.orgId };
  if (route.meTime) return { ...NOWHERE, meTime: true };
  const readToken = route.readToken || undefined;
  const workspaceId = (!readToken && route.workspaceId) || undefined;
  const wikiId = (workspaceId && route.wikiId) || undefined;
  if (wikiId) return { ...NOWHERE, workspaceId, wikiId, pageId: route.pageId || undefined };
  const diaryId = (workspaceId && route.diaryId) || undefined;
  if (diaryId) {
    const monthDay = route.monthDay || undefined;
    const year = monthDay ? diaryYear(route.year) : undefined;
    return { ...NOWHERE, workspaceId, diaryId, monthDay, ...(year ? { year } : {}) };
  }
  const courseId = (workspaceId && route.courseId) || undefined;
  if (courseId) {
    const courseTab =
      route.courseTab === 'meetings' || route.courseTab === 'exams' ? route.courseTab : undefined;
    const meetingId = courseTab === 'meetings' ? route.meetingId || undefined : undefined;
    return {
      ...NOWHERE,
      workspaceId,
      courseId,
      unitId: route.unitId || undefined,
      ...(courseTab ? { courseTab } : {}),
      ...(meetingId ? { meetingId } : {}),
    };
  }
  if (workspaceId && route.reviewsToday) return { ...NOWHERE, workspaceId, reviewsToday: true };
  if (workspaceId && route.learningToday) return { ...NOWHERE, workspaceId, learningToday: true };
  const boardId = ((workspaceId || readToken) && route.boardId) || undefined;
  const viewId = (boardId && route.viewId) || undefined;
  const cardId = (viewId && route.cardId) || undefined;
  if (readToken && boardId)
    return { workspaceId, boardId, viewId, cardId, readToken, unknown: false, shared: true };
  return { workspaceId, boardId, viewId, cardId, unknown: false };
}

/**
 * The route of a board, or of a wiki, diary or course where it is one (`kind`).
 */
export function placeRoute(container: ContainerPlace): Partial<Route> {
  const { workspaceId, id } = container;
  if (container.kind === 'wiki') return { workspaceId, wikiId: id };
  if (container.kind === 'diary') return { workspaceId, diaryId: id };
  if (container.kind === 'course') return { workspaceId, courseId: id };
  return { workspaceId, boardId: id };
}

/** The board, wiki, diary or course a route shows. */
export function shownContainerId(route: Route): string | undefined {
  return route.boardId ?? route.wikiId ?? route.diaryId ?? route.courseId;
}

/** The address of a route, from the root of the site. */
export function routePath(route: Partial<Route>, basePath: string): string {
  if (route.orgId) return buildUrl(`/org/${encodeURIComponent(route.orgId)}`, basePath);
  if (route.meTime) return buildUrl('/me/time', basePath);
  const {
    workspaceId,
    boardId,
    viewId,
    cardId,
    readToken,
    wikiId,
    pageId,
    diaryId,
    monthDay,
    year,
    courseId,
    unitId,
    courseTab,
    meetingId,
    reviewsToday,
    learningToday,
  } = settled(route);
  if (readToken && boardId) {
    const parts = [boardId, viewId, cardId].filter((part) => part !== undefined);
    const path = ['shared', ...parts.map((part) => encodeURIComponent(part))].join('/');
    return `${buildUrl(`/${path}`, basePath)}?r=${encodeURIComponent(readToken)}`;
  }
  if (!workspaceId) return buildUrl('/', basePath);
  if (reviewsToday)
    return buildUrl(`/w/${encodeURIComponent(workspaceId)}/learning/reviews/today`, basePath);
  if (learningToday)
    return buildUrl(`/w/${encodeURIComponent(workspaceId)}/learning/today`, basePath);
  const named = (
    wikiId
      ? [wikiId, pageId]
      : diaryId
        ? [diaryId, monthDay]
        : courseId
          ? [courseId, unitId]
          : [boardId, viewId, cardId]
  ).filter((part) => part !== undefined);
  const section = wikiId ? 'wikis' : diaryId ? 'diaries' : courseId ? 'courses' : 'boards';
  const path = [
    'w',
    encodeURIComponent(workspaceId),
    ...(named.length > 0 ? [section] : []),
    ...named.map((part) => encodeURIComponent(part)),
  ];
  const url = buildUrl(`/${path.join('/')}`, basePath);
  if (courseId && courseTab) {
    const focus =
      courseTab === 'meetings' && meetingId ? `&meeting=${encodeURIComponent(meetingId)}` : '';
    return `${url}?tab=${encodeURIComponent(courseTab)}${focus}`;
  }
  return diaryId && monthDay && year ? `${url}?year=${encodeURIComponent(year)}` : url;
}
