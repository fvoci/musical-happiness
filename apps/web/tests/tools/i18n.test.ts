/**
 * The message files (`messages/<locale>.json`) and the comparison of their
 * key sets (tools/i18n/keys.ts). The message files are the source: they are
 * edited by hand, and paraglide compiles them.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { COMPLETE_LOCALES, compareKeys, keyErrors, messageKeys } from '../../tools/i18n/keys.ts';

const WEB_ROOT = path.resolve(import.meta.dirname, '../..');

describe('key sets', () => {
  it('names what is missing and what is extra, in the order of the files', () => {
    expect(compareKeys(['a', 'b', 'c', 'd'], ['d', 'x', 'b', 'y'])).toEqual({
      missing: ['a', 'c'],
      extra: ['x', 'y'],
    });
  });

  it('finds no difference between equal sets in a different order', () => {
    expect(compareKeys(['a', 'b'], ['b', 'a'])).toEqual({ missing: [], extra: [] });
  });

  it('reads ids with dots as they are, and a complex message as one key', () => {
    const file = {
      $schema: 'https://inlang.com/schema/inlang-message-format',
      ui_close: 'Close',
      'Sidebar.delete-board': 'Delete board',
      'Sidebar.delete-board.confirm': 'Sure?',
      count: [{ match: { 'countPlural=one': 'one', 'countPlural=*': 'many' } }],
      nested: { title: 'Title', deeper: { text: 'Text' } },
    };
    expect(messageKeys(file)).toEqual([
      'ui_close',
      'Sidebar.delete-board',
      'Sidebar.delete-board.confirm',
      'count',
      'nested.title',
      'nested.deeper.text',
    ]);
  });

  it('accepts a locale that lacks keys, unless it has to be complete', () => {
    const differences = new Map([
      ['ko', { missing: [], extra: [] }],
      ['de', { missing: ['a', 'b'], extra: [] }],
    ]);
    expect(keyErrors(differences, ['ko'])).toEqual([]);
    expect(keyErrors(differences, ['ko', 'de'])).toEqual([
      'de: 2 key(s) missing; this locale has to be complete',
    ]);
  });

  it('refuses a key the base locale does not have, in any locale', () => {
    const differences = new Map([
      ['ko', { missing: [], extra: [] }],
      ['de', { missing: [], extra: ['stale'] }],
    ]);
    expect(keyErrors(differences, ['ko'])).toEqual(['de: 1 key(s) the base locale does not have']);
  });

  it('refuses a complete locale that is not there at all', () => {
    expect(keyErrors(new Map(), ['ko'])).toEqual(['ko: has to be complete but is not a locale']);
  });
});

describe('the message files', () => {
  const settings = JSON.parse(
    readFileSync(path.join(WEB_ROOT, 'project.inlang/settings.json'), 'utf8'),
  ) as { baseLocale: string; locales: string[] };
  const messages = (locale: string): Record<string, unknown> =>
    JSON.parse(readFileSync(path.join(WEB_ROOT, `messages/${locale}.json`), 'utf8')) as Record<
      string,
      unknown
    >;

  it('are those of the eighteen locales, English first and Korean among them', () => {
    expect(settings.baseLocale).toBe('en');
    expect(settings.locales).toHaveLength(18);
    expect(settings.locales[0]).toBe('en');
    for (const locale of COMPLETE_LOCALES) expect(settings.locales).toContain(locale);
    for (const locale of settings.locales)
      expect(messageKeys(messages(locale)).length).toBeGreaterThan(0);
  });

  it('give Korean every key of English and no locale a key English lacks', () => {
    const english = messageKeys(messages('en'));
    expect(compareKeys(english, messageKeys(messages('ko')))).toEqual({ missing: [], extra: [] });
    for (const locale of settings.locales) {
      expect(compareKeys(english, messageKeys(messages(locale))).extra, locale).toEqual([]);
    }
  });

  it('name the product where the English text named it "Boards", and only there', () => {
    for (const id of [
      'BoardPage.newVersion',
      'OnboardingTour.OpenACard.Body',
      'error.invalid-read-only-board',
      'error.not-logged-in',
    ]) {
      expect(messages('en')[id]).toContain('fvoci');
      expect(messages('en')[id]).not.toContain('Boards');
      expect(messages('ko')[id]).toContain('fvoci');
    }
    // The default category of the sidebar is called "Boards"; that stays.
    expect(messages('en')['SidebarCategories.CategoryMenu.DeleteModal.Body']).toContain(
      'the Boards categories',
    );
  });

  // A Korean particle depends on how the word before it ends (을/를, 이/가, …),
  // so it cannot follow a value that is put in at run time: `{name}` or the end
  // of a tag, `{/b}`.
  const particleAfterValue = /\}["”]?(?:은|는|이|가|을|를|로|으로|와|과)(?:\s|$)/;

  it('find a Korean particle right after a value', () => {
    expect(particleAfterValue.test('{name}을 지웁니다')).toBe(true);
    expect(particleAfterValue.test('{#b}{name}{/b}를 옮깁니다')).toBe(true);
    expect(particleAfterValue.test('"{name}" 보드')).toBe(false);
  });

  it('have no Korean particle right after a value', () => {
    const risky = Object.entries(messages('ko')).filter(
      ([, text]) => typeof text === 'string' && particleAfterValue.test(text),
    );
    expect(risky.map(([id]) => id)).toEqual([]);
  });
});
