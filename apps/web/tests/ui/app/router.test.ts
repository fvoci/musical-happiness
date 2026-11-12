/** Addresses: a board, a view and a card under the base path the server names. */
import { beforeEach, describe, expect, it, vi } from 'vitest';

import * as back from '$lib/layout/back.svelte.ts';

import { localeOfSetting, matchLocale } from '../../../src/app/locale.ts';
import { parseRoute, readBasePath, routePath, Router } from '../../../src/app/router.svelte.ts';

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
    router.openBoard({ id: 'other', workspaceId: 't' });
    expect(location.pathname).toBe('/w/t/boards/other');
    expect(router.route).toMatchObject({ boardId: 'other', viewId: undefined, cardId: undefined });
  });

  it('opens a board at the address of its own workspace', () => {
    const router = new Router('');
    router.openBoard({ id: 'theirs', workspaceId: 't2' });
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

  it('loads the page anew at a route, and what is open over it gives no step back', () => {
    const router = new Router('app');
    const assign = vi.fn();
    vi.stubGlobal('location', { assign });
    try {
      const released = vi.spyOn(back, 'releaseOverlayEntries');
      router.restart({});
      expect(released).toHaveBeenCalledOnce();
      expect(assign).toHaveBeenCalledWith('/app/');
    } finally {
      vi.unstubAllGlobals();
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
