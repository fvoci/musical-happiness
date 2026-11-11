/** Addresses: a board, a view and a card under the base path the server names. */
import { beforeEach, describe, expect, it } from 'vitest';

import { localeOfSetting, matchLocale } from '../../../src/app/locale.ts';
import { parseRoute, readBasePath, routePath, Router } from '../../../src/app/router.svelte.ts';

const none = { boardId: undefined, viewId: undefined, cardId: undefined, unknown: false };

describe('reading an address', () => {
  it('reads board, view and card, each optional from the right', () => {
    expect(parseRoute('/', '')).toEqual(none);
    expect(parseRoute('/board', '')).toEqual(none);
    expect(parseRoute('/board/b', '')).toEqual({ ...none, boardId: 'b' });
    expect(parseRoute('/board/b/v', '')).toEqual({ ...none, boardId: 'b', viewId: 'v' });
    expect(parseRoute('/board/b/v/c', '')).toEqual({
      boardId: 'b',
      viewId: 'v',
      cardId: 'c',
      unknown: false,
    });
    expect(parseRoute('/board/b/v/', '')).toEqual({ ...none, boardId: 'b', viewId: 'v' });
  });

  it('does not know other addresses', () => {
    expect(parseRoute('/login', '').unknown).toBe(true);
    expect(parseRoute('/board/b/v/c/more', '').unknown).toBe(true);
  });

  it('works under a base path, and knows nothing outside of it', () => {
    expect(parseRoute('/boards', 'boards')).toEqual(none);
    expect(parseRoute('/boards/board/b', 'boards')).toEqual({ ...none, boardId: 'b' });
    expect(parseRoute('/elsewhere/board/b', 'boards').unknown).toBe(true);
    expect(parseRoute('/boardsx/board/b', 'boards').unknown).toBe(true);
  });

  it('decodes what the address encodes, and survives what does not decode', () => {
    expect(parseRoute('/board/a%20b', '').boardId).toBe('a b');
    expect(parseRoute('/board/%E0%A4%A', '').boardId).toBe('%E0%A4%A');
  });
});

describe('writing an address', () => {
  it('is the reverse of reading one', () => {
    for (const path of ['/', '/board/b', '/board/b/v', '/board/b/v/c']) {
      expect(routePath(parseRoute(path, ''), '')).toBe(path);
    }
    expect(routePath({ boardId: 'b', viewId: 'v' }, 'boards')).toBe('/boards/board/b/v');
    expect(routePath({}, 'boards')).toBe('/boards/');
  });

  it('drops a card without a view', () => {
    expect(routePath({ boardId: 'b', cardId: 'c' }, '')).toBe('/board/b');
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
    history.replaceState(null, '', '/board/b/v');
  });

  it('starts from the address the page was loaded on', () => {
    expect(new Router('').route).toMatchObject({ boardId: 'b', viewId: 'v', cardId: undefined });
  });

  it('opens a card as a new step and another card in its place', () => {
    const router = new Router('');
    const before = history.length;
    router.openCard('c1');
    expect(location.pathname).toBe('/board/b/v/c1');
    expect(history.length).toBe(before + 1);
    router.openCard('c2');
    expect(location.pathname).toBe('/board/b/v/c2');
    expect(history.length).toBe(before + 1);
  });

  it('closes a card it was loaded on by replacing the address', () => {
    history.replaceState(null, '', '/board/b/v/c');
    const router = new Router('');
    const before = history.length;
    router.closeCard();
    expect(location.pathname).toBe('/board/b/v');
    expect(history.length).toBe(before);
    expect(router.route.cardId).toBeUndefined();
  });

  it('drops the view and the card when another board is opened', () => {
    const router = new Router('');
    router.openCard('c');
    router.openBoard('other');
    expect(location.pathname).toBe('/board/other');
    expect(router.route).toMatchObject({ boardId: 'other', viewId: undefined, cardId: undefined });
  });

  it('makes links that keep the base path', () => {
    expect(new Router('boards').href({ boardId: 'b', viewId: 'v', cardId: 'c' })).toBe(
      '/boards/board/b/v/c',
    );
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
