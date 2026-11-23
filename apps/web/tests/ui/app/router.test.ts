/** Addresses: a board, a view and a card under the base path the server names. */
import { beforeEach, describe, expect, it, vi } from 'vitest';

import * as back from '$lib/layout/back.svelte.ts';

import { localeOfSetting, matchLocale } from '../../../src/app/locale.ts';
import { goToPlace, routeOfPlace } from '../../../src/app/places.ts';
import {
  parseRoute,
  placeRoute,
  readBasePath,
  routePath,
  Router,
  shownContainerId,
} from '../../../src/app/router.svelte.ts';

const none = {
  workspaceId: undefined,
  boardId: undefined,
  viewId: undefined,
  cardId: undefined,
  unknown: false,
};

describe('reading an address', () => {
  it('reads workspace, board, view and card, each optional from the right', () => {
    expect(parseRoute('/', '')).toEqual(none);
    expect(parseRoute('/w/t', '')).toEqual({ ...none, workspaceId: 't' });
    expect(parseRoute('/w/t/boards', '')).toEqual({ ...none, workspaceId: 't' });
    expect(parseRoute('/w/t/boards/b', '')).toEqual({ ...none, workspaceId: 't', boardId: 'b' });
    expect(parseRoute('/w/t/boards/b/v', '')).toEqual({
      ...none,
      workspaceId: 't',
      boardId: 'b',
      viewId: 'v',
    });
    expect(parseRoute('/w/t/boards/b/v/c', '')).toEqual({
      workspaceId: 't',
      boardId: 'b',
      viewId: 'v',
      cardId: 'c',
      unknown: false,
    });
    expect(parseRoute('/w/t/boards/b/v/', '')).toMatchObject({ boardId: 'b', viewId: 'v' });
  });

  it('reads a public link: the board, a view and a card, and the read token', () => {
    expect(parseRoute('/base/shared/b/v/c', 'base', '?r=tok')).toEqual({
      ...none,
      boardId: 'b',
      viewId: 'v',
      cardId: 'c',
      readToken: 'tok',
      shared: true,
    });
    expect(parseRoute('/shared/b', '', '?r=a%2Fb')).toEqual({
      ...none,
      boardId: 'b',
      readToken: 'a/b',
      shared: true,
    });
    // Without a token, or with more than a card, it is no address of the app's.
    expect(parseRoute('/shared/b', '').unknown).toBe(true);
    expect(parseRoute('/shared/b', '', '?r=').unknown).toBe(true);
    expect(parseRoute('/shared', '', '?r=t').unknown).toBe(true);
    expect(parseRoute('/shared/b/v/c/x', '', '?r=t').unknown).toBe(true);
    // Each is still under `/shared`: the reader's screen says not found, not the app.
    for (const [path, search] of [
      ['/shared/b', ''],
      ['/shared/b', '?r='],
      ['/shared', '?r=t'],
      ['/shared/b/v/c/x', '?r=t'],
    ] as const) {
      expect(parseRoute(path, '', search).shared).toBe(true);
    }
    expect(parseRoute('/w/t', '').shared).toBeUndefined();
  });

  it('writes a public link under the base path, the token in the query', () => {
    const shared = { boardId: 'b', viewId: 'v', readToken: 't/1' };
    expect(routePath(shared, 'base')).toBe('/base/shared/b/v?r=t%2F1');
    expect(routePath({ ...shared, cardId: 'c' }, '')).toBe('/shared/b/v/c?r=t%2F1');
    // A link names no workspace, also when the route has one.
    expect(routePath({ ...shared, workspaceId: 'w' }, '')).toBe('/shared/b/v?r=t%2F1');
  });

  it("reads the organization's screen, and nothing below it", () => {
    expect(parseRoute('/org/o', '')).toEqual({ ...none, orgId: 'o' });
    expect(parseRoute('/app/org/o', 'app')).toEqual({ ...none, orgId: 'o' });
    expect(parseRoute('/org', '').unknown).toBe(true);
    expect(parseRoute('/org/o/w', '').unknown).toBe(true);
    expect(routePath({ orgId: 'o 1' }, 'app')).toBe('/app/org/o%201');
  });

  it('does not know other addresses, the old ones among them', () => {
    expect(parseRoute('/login', '').unknown).toBe(true);
    expect(parseRoute('/w', '').unknown).toBe(true);
    expect(parseRoute('/w/t/bord/b', '').unknown).toBe(true);
    expect(parseRoute('/w/t/boards/b/v/c/more', '').unknown).toBe(true);
    expect(parseRoute(['', 'board', 'b', 'v', 'c'].join('/'), '').unknown).toBe(true);
  });

  it('works under a base path, and knows nothing outside of it', () => {
    expect(parseRoute('/app', 'app')).toEqual(none);
    expect(parseRoute('/app/w/t/boards/b', 'app')).toMatchObject({
      workspaceId: 't',
      boardId: 'b',
    });
    expect(parseRoute('/elsewhere/w/t/boards/b', 'app').unknown).toBe(true);
    expect(parseRoute('/appx/w/t/boards/b', 'app').unknown).toBe(true);
  });

  it('decodes what the address encodes, and survives what does not decode', () => {
    expect(parseRoute('/w/a%2Fb/boards/a%20b', '')).toMatchObject({
      workspaceId: 'a/b',
      boardId: 'a b',
    });
    expect(parseRoute('/w/t/boards/%E0%A4%A', '').boardId).toBe('%E0%A4%A');
  });
});

describe('writing an address', () => {
  it('is the reverse of reading one', () => {
    for (const path of [
      '/',
      '/w/t',
      '/w/t/boards/b',
      '/w/t/boards/b/v',
      '/w/t/boards/b/v/c',
      '/w/a%2Fb/boards/a%20b%3F/v%23',
    ]) {
      expect(routePath(parseRoute(path, ''), '')).toBe(path);
    }
    expect(routePath({ workspaceId: 't', boardId: 'b', viewId: 'v' }, 'app')).toBe(
      '/app/w/t/boards/b/v',
    );
    expect(routePath({}, 'app')).toBe('/app/');
  });

  it('drops what has nothing to stand under: a card without a view, a board without a workspace', () => {
    expect(routePath({ workspaceId: 't', boardId: 'b', cardId: 'c' }, '')).toBe('/w/t/boards/b');
    expect(routePath({ boardId: 'b', viewId: 'v' }, '')).toBe('/');
  });
});

describe('the address of a diary', () => {
  it('reads the diary and the day of the year, and writes them back', () => {
    expect(parseRoute('/w/me/diaries/d/11-21', '')).toMatchObject({
      workspaceId: 'me',
      diaryId: 'd',
      monthDay: '11-21',
      boardId: undefined,
      unknown: false,
    });
    expect(parseRoute('/w/me/diaries/d', '')).toMatchObject({ diaryId: 'd', monthDay: undefined });
    for (const path of ['/w/me/diaries/d', '/w/me/diaries/d/02-29']) {
      expect(routePath(parseRoute(path, ''), '')).toBe(path);
    }
  });

  it('reads and writes ?year=YYYY on a diary day, ignoring invalid years', () => {
    expect(parseRoute('/w/me/diaries/d/11-21', '', '?year=2024')).toMatchObject({
      workspaceId: 'me',
      diaryId: 'd',
      monthDay: '11-21',
      year: '2024',
      unknown: false,
    });
    expect(routePath(parseRoute('/w/me/diaries/d/11-21', '', '?year=2024'), '')).toBe(
      '/w/me/diaries/d/11-21?year=2024',
    );
    expect(routePath({ workspaceId: 'me', diaryId: 'd', year: '2024' }, '')).toBe(
      '/w/me/diaries/d',
    );
    for (const bad of ['', '?year=', '?year=abc', '?year=0000', '?year=20250', '?year=-1']) {
      expect(parseRoute('/w/me/diaries/d/11-21', '', bad).year).toBeUndefined();
    }
    expect(parseRoute('/w/me/diaries/d', '', '?year=2024').year).toBeUndefined();
  });

  it('builds entry search routes with ?year=YYYY and follows popstate across years', () => {
    history.replaceState(null, '', '/w/me/diaries/d/11-21');
    const router = new Router('');
    const stop = router.listen();
    const hit = {
      kind: 'entry' as const,
      id: 'e1',
      containerId: 'd',
      workspaceId: 'me',
      title: '2024-11-21',
    };
    expect(routeOfPlace(router, hit)).toEqual({
      workspaceId: 'me',
      diaryId: 'd',
      monthDay: '11-21',
      year: '2024',
    });
    expect(router.href(routeOfPlace(router, hit))).toBe('/w/me/diaries/d/11-21?year=2024');
    goToPlace(router, hit);
    expect(`${location.pathname}${location.search}`).toBe('/w/me/diaries/d/11-21?year=2024');
    expect(router.route.year).toBe('2024');
    router.openDay('11-21');
    expect(`${location.pathname}${location.search}`).toBe('/w/me/diaries/d/11-21');
    expect(router.route.year).toBeUndefined();
    history.replaceState(null, '', '/w/me/diaries/d/11-21?year=2023');
    window.dispatchEvent(new PopStateEvent('popstate'));
    expect(router.route.year).toBe('2023');
    stop();
  });

  it('keeps a day no year has, for the page to say it is not found', () => {
    expect(parseRoute('/w/me/diaries/d/13-40', '')).toMatchObject({ monthDay: '13-40' });
  });

  it('is the address of a diary in the sidebar, and its diary the container shown', () => {
    const diary = { id: 'd', workspaceId: 'me', kind: 'diary' as const };
    expect(placeRoute(diary)).toEqual({ workspaceId: 'me', diaryId: 'd' });
    expect(shownContainerId(parseRoute('/w/me/diaries/d/11-21', ''))).toBe('d');
  });

  it('goes from day to day, the diary kept, and back to the day before', () => {
    history.replaceState(null, '', '/w/me/diaries/d/11-21');
    const router = new Router('');
    const stop = router.listen();
    const before = history.length;
    router.openDay('11-22');
    expect(router.route).toMatchObject({ diaryId: 'd', monthDay: '11-22' });
    expect(location.pathname).toBe('/w/me/diaries/d/11-22');
    expect(history.length).toBe(before + 1);
    router.openDay('11-23', { replace: true });
    expect(history.length).toBe(before + 1);
    // Back to another day of the same diary is a move too.
    history.replaceState(null, '', '/w/me/diaries/d/11-21');
    window.dispatchEvent(new PopStateEvent('popstate'));
    expect(router.route.monthDay).toBe('11-21');
    stop();
  });
});

describe('the address of a course', () => {
  it('reads the course and its unit, writes them back, and rejects extra segments', () => {
    const parsed = parseRoute('/w/ws/courses/c1/u1', '');
    expect(parsed).toMatchObject({
      workspaceId: 'ws',
      courseId: 'c1',
      unitId: 'u1',
      unknown: false,
    });
    expect(parsed.boardId).toBeUndefined();
    expect(parsed.wikiId).toBeUndefined();
    expect(parseRoute('/w/ws/courses/c1', '')).toMatchObject({
      workspaceId: 'ws',
      courseId: 'c1',
      unitId: undefined,
      unknown: false,
    });
    expect(parseRoute('/w/ws/courses/c1/u1/extra', '').unknown).toBe(true);
    for (const path of ['/w/ws/courses/c1', '/w/ws/courses/c1/u1']) {
      expect(routePath(parseRoute(path, ''), '')).toBe(path);
    }
    expect(routePath({ workspaceId: 'ws', unitId: 'u1' }, '')).toBe('/w/ws');
  });

  it('places courses in the sidebar, resolves shownContainerId, and navigates units and search hits', () => {
    const course = { id: 'c1', workspaceId: 'ws', kind: 'course' as const };
    expect(placeRoute(course)).toEqual({ workspaceId: 'ws', courseId: 'c1' });
    expect(shownContainerId(parseRoute('/w/ws/courses/c1/u1', ''))).toBe('c1');

    history.replaceState(null, '', '/w/ws/courses/c1');
    const router = new Router('');
    const stop = router.listen();
    const before = history.length;
    router.openUnit('u1');
    expect(router.route).toMatchObject({ courseId: 'c1', unitId: 'u1' });
    expect(location.pathname).toBe('/w/ws/courses/c1/u1');
    expect(history.length).toBe(before + 1);

    router.openUnit('u2', { replace: true });
    expect(router.route.unitId).toBe('u2');
    expect(history.length).toBe(before + 1);

    const hit = {
      kind: 'unit' as const,
      id: 'u9',
      containerId: 'c2',
      workspaceId: 'ws',
      title: 'Lesson 9',
    };
    expect(routeOfPlace(router, hit)).toEqual({
      workspaceId: 'ws',
      courseId: 'c2',
      unitId: 'u9',
    });
    goToPlace(router, hit);
    expect(location.pathname).toBe('/w/ws/courses/c2/u9');
    expect(router.route).toMatchObject({ courseId: 'c2', unitId: 'u9' });
    stop();
  });
});

describe('the base path', () => {
  const metaWith = (content: string): Document => {
    const other = document.implementation.createHTMLDocument('');
    const meta = other.createElement('meta');
    meta.setAttribute('name', 'fvoci:base');
    meta.setAttribute('content', content);
    other.head.append(meta);
    return other;
  };

  it('is what the server wrote into the page', () => {
    expect(readBasePath(metaWith('/boards'))).toBe('boards');
    expect(readBasePath(metaWith('/boards/'))).toBe('boards');
    expect(readBasePath(metaWith(''))).toBe('');
  });

  it('is empty when nobody filled the placeholder in, or the element is missing', () => {
    expect(readBasePath(metaWith('{{.BaseURL}}'))).toBe('');
    expect(readBasePath(document.implementation.createHTMLDocument(''))).toBe('');
  });
});

describe('moving about', () => {
  beforeEach(() => {
    history.replaceState(null, '', '/w/t/boards/b/v');
  });

  it('starts from the address the page was loaded on', () => {
    expect(new Router('').route).toMatchObject({
      workspaceId: 't',
      boardId: 'b',
      viewId: 'v',
      cardId: undefined,
    });
  });

  it('opens a card as a new step and another card in its place', () => {
    const router = new Router('');
    const before = history.length;
    router.openCard('c1');
    expect(location.pathname).toBe('/w/t/boards/b/v/c1');
    expect(history.length).toBe(before + 1);
    router.openCard('c2');
    expect(location.pathname).toBe('/w/t/boards/b/v/c2');
    expect(history.length).toBe(before + 1);
  });

  it('closes a card it was loaded on by replacing the address', () => {
    history.replaceState(null, '', '/w/t/boards/b/v/c');
    const router = new Router('');
    const before = history.length;
    router.closeCard();
    expect(location.pathname).toBe('/w/t/boards/b/v');
    expect(history.length).toBe(before);
    expect(router.route.cardId).toBeUndefined();
  });

  it('drops the view and the card when another board is opened', () => {
    const router = new Router('');
    router.openCard('c');
    router.openContainer({ id: 'other', workspaceId: 't' });
    expect(location.pathname).toBe('/w/t/boards/other');
    expect(router.route).toMatchObject({ boardId: 'other', viewId: undefined, cardId: undefined });
  });

  it('opens a board at the address of its own workspace', () => {
    const router = new Router('');
    router.openContainer({ id: 'theirs', workspaceId: 't2' });
    expect(location.pathname).toBe('/w/t2/boards/theirs');
    expect(router.route).toMatchObject({ workspaceId: 't2', boardId: 'theirs' });
  });

  it('keeps the workspace when another view is opened', () => {
    const router = new Router('');
    router.openView('v2');
    expect(location.pathname).toBe('/w/t/boards/b/v2');
  });

  it('follows back and forward to another workspace', () => {
    const router = new Router('');
    const stop = router.listen();
    history.pushState(null, '', '/w/t2/boards/x');
    window.dispatchEvent(new PopStateEvent('popstate'));
    expect(router.route).toMatchObject({ workspaceId: 't2', boardId: 'x' });
    stop();
  });

  it('loads the page anew at a route, the entries of what is open let go first', () => {
    const router = new Router('app');
    const calls: string[] = [];
    const released = vi.spyOn(back, 'releaseOverlayEntries');
    vi.stubGlobal('location', {
      assign: (to: string) => calls.push(`assign ${to}`),
      replace: (to: string) => calls.push(`replace ${to}`),
    });
    try {
      // Nothing open: a new entry, as any going somewhere.
      released.mockImplementationOnce(() => {
        calls.push('release');
        return 0;
      });
      router.restart({});
      // A dialog open, with its entry: the new address takes that entry's place.
      released.mockImplementationOnce(() => {
        calls.push('release');
        return 1;
      });
      router.restart({});
      expect(calls).toEqual(['release', 'assign /app/', 'release', 'replace /app/']);
    } finally {
      vi.unstubAllGlobals();
      released.mockRestore();
    }
  });

  it('makes links that keep the base path', () => {
    expect(
      new Router('app').href({ workspaceId: 't', boardId: 'b', viewId: 'v', cardId: 'c' }),
    ).toBe('/app/w/t/boards/b/v/c');
  });
});

describe('the language', () => {
  it('finds the closest set of messages for what the browser asks', () => {
    expect(matchLocale('ko-KR')).toBe('ko');
    expect(matchLocale('en-GB')).toBe('en');
    expect(matchLocale('zh-TW')).toBe('zh-Hant');
    expect(matchLocale('zh-Hant-HK')).toBe('zh-Hant');
    expect(matchLocale('zh-CN')).toBe('zh-Hans');
    expect(matchLocale('pt-PT')).toBe('pt-BR');
    expect(matchLocale('xx-YY')).toBeUndefined();
    // The server answers a tag as it was given, case and all.
    expect(matchLocale('PT-br')).toBe('pt-BR');
    expect(matchLocale('ZH-hant')).toBe('zh-Hant');
  });

  it('shows the account setting in its closest set, and auto or one it has none for as the browser asks', () => {
    const browser = matchLocale(navigator.languages[0] ?? '') ?? 'en';
    expect(localeOfSetting('ko')).toBe('ko');
    expect(localeOfSetting('de-AT')).toBe('de');
    expect(localeOfSetting('auto')).toBe(browser);
    expect(localeOfSetting('xx')).toBe(browser);
  });
});
