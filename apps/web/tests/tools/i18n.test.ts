/**
 * The tools of tools/i18n: the comparison of key sets, the ICU parser, the
 * conversion of each construct to the inlang message format, and what the
 * converted messages return once paraglide has compiled them.
 */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

import { compile } from '@inlang/paraglide-js';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { convertAll, convertMessage, renameProduct } from '../../tools/i18n/conversion.ts';
import { IcuSyntaxError, parseIcu } from '../../tools/i18n/icu.ts';
import { type InlangMessage, shapeOf, toInlang } from '../../tools/i18n/inlang.ts';
import { compareKeys, keyErrors, messageKeys } from '../../tools/i18n/keys.ts';
import { bundledFiles, LOCALES, WEB_ROOT } from '../../tools/i18n/locales.ts';

const convert = (icu: string): InlangMessage => toInlang(parseIcu(icu));

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

describe('ICU parser', () => {
  it.each([
    ["It's here", "It's here"],
    ["a '' b", "a ' b"],
    ["'{'x'}'", '{x}'],
    ["'{x} and the rest", '{x} and the rest'],
    ["don't '<b>' it", "don't <b> it"],
    ['a } b', 'a } b'],
    ['1 < 2 <3', '1 < 2 <3'],
    ['line<br/>break<br />', 'line<br/>break<br/>'],
    ['# of cards', '# of cards'],
  ])('reads %j as the text %j', (icu, text) => {
    expect(parseIcu(icu)).toEqual([{ type: 'text', value: text }]);
  });

  it('reads arguments, tags and what is inside them', () => {
    expect(parseIcu('Boards in <b>{ name }</b>!')).toEqual([
      { type: 'text', value: 'Boards in ' },
      { type: 'tag', name: 'b', children: [{ type: 'argument', name: 'name' }] },
      { type: 'text', value: '!' },
    ]);
  });

  it('reads a plural with an offset, exact numbers and the number sign', () => {
    expect(parseIcu('{n, plural, offset:1 =0 {none} one {# more} other {#}}')).toEqual([
      {
        type: 'plural',
        name: 'n',
        ordinal: false,
        offset: 1,
        options: [
          { selector: '=0', value: [{ type: 'text', value: 'none' }] },
          { selector: 'one', value: [{ type: 'pound' }, { type: 'text', value: ' more' }] },
          { selector: 'other', value: [{ type: 'pound' }] },
        ],
      },
    ]);
  });

  it('takes the number sign for text inside a select, also within a plural', () => {
    const [plural] = parseIcu('{n, plural, other {{g, select, other {#}}}}');
    expect(plural).toMatchObject({
      options: [
        { value: [{ type: 'select', options: [{ value: [{ type: 'text', value: '#' }] }] }] },
      ],
    });
  });

  it('reads the style of a number, a date and a time', () => {
    expect(parseIcu('{a, number}{b, number, percent}{c, date, short}{d, time, ::Hm}')).toEqual([
      { type: 'format', name: 'a', format: 'number', style: undefined },
      { type: 'format', name: 'b', format: 'number', style: 'percent' },
      { type: 'format', name: 'c', format: 'date', style: 'short' },
      { type: 'format', name: 'd', format: 'time', style: '::Hm' },
    ]);
  });

  it.each([
    ['{', 'EXPECT_ARGUMENT_CLOSING_BRACE'],
    ['{}', 'EMPTY_ARGUMENT'],
    ['{user name}', 'MALFORMED_ARGUMENT'],
    ['{name', 'EXPECT_ARGUMENT_CLOSING_BRACE'],
    ['{n, list}', 'INVALID_ARGUMENT_TYPE'],
    ['{n, plural}', 'EXPECT_SELECT_ARGUMENT_OPTIONS'],
    ['{n, plural, one {a}}', 'MISSING_OTHER_CLAUSE'],
    ['{n, plural, one {a} one {b} other {c}}', 'DUPLICATE_ARGUMENT_SELECTOR'],
    ['{g, select, =1 {a} other {b}}', 'EXPECT_ARGUMENT_SELECTOR'],
    ['a </b>', 'UNMATCHED_CLOSING_TAG'],
    ['<b>bold', 'UNCLOSED_TAG'],
    ['<b>bold</i>', 'UNMATCHED_CLOSING_TAG'],
    ['<b bold</b>', 'INVALID_TAG'],
  ])('refuses %j: %s', (icu, kind) => {
    let caught: unknown;
    try {
      parseIcu(icu);
    } catch (error) {
      caught = error;
    }
    expect(caught).toBeInstanceOf(IcuSyntaxError);
    expect((caught as IcuSyntaxError).kind).toBe(kind);
  });

  it('tells which arguments and tags a message has', () => {
    const shape = shapeOf(parseIcu('<b>{a}</b> {n, plural, one {{c}} other {# {a}}}'));
    expect([...shape.arguments]).toEqual(['a', 'n', 'c']);
    expect([...shape.tags]).toEqual(['b']);
    expect(Object.fromEntries(shape.constructs)).toEqual({
      tag: 1,
      argument: 3,
      plural: 1,
      '#': 1,
    });
  });
});

describe('conversion to the inlang message format', () => {
  it.each([
    ['text', 'Delete board', 'Delete board'],
    ['an apostrophe', "You're editing a template.", "You're editing a template."],
    ['an escaped apostrophe', "a '' b", "a ' b"],
    ['quoted braces', "'{'x'}' } \\", '\\{x\\} \\} \\\\'],
    ['an argument', 'No { property }', 'No {property}'],
    ['a tag', 'Use <b>UP/DOWN</b> to browse {n}', 'Use {#b}UP/DOWN{/b} to browse {n}'],
    ['a tag around an argument', '<b>{name}</b> moves', '{#b}{name}{/b} moves'],
    ['a self-closing tag, which is text', 'a<br/>b', 'a<br/>b'],
    ['a number', '{n, number}', '{n: number}'],
    ['a whole number', '{n, number, integer}', '{n: number style=integer}'],
    ['a percentage', '{n, number, percent}', '{n: number style=percent}'],
    ['a plural with one clause', '{n, plural, other {# cards}}', '{n: icu:pound} cards'],
  ])('%s', (_, icu, pattern) => {
    expect(convert(icu)).toBe(pattern);
  });

  it('a plural', () => {
    expect(convert('{count, plural, one {# card} other {# cards}} hidden')).toEqual([
      {
        declarations: ['input count', 'local countPlural = count: plural'],
        selectors: ['countPlural'],
        match: {
          'countPlural=one': '{count: icu:pound} card hidden',
          'countPlural=*': '{count: icu:pound} cards hidden',
        },
      },
    ]);
  });

  it('a plural with exact numbers, which come first', () => {
    expect(convert('{count, plural, one {one} other {many} =0 {none}}')).toEqual([
      {
        declarations: [
          'input count',
          'local countPluralExact = count',
          'local countPlural = count: plural',
        ],
        selectors: ['countPluralExact', 'countPlural'],
        match: {
          'countPluralExact=0, countPlural=*': 'none',
          'countPluralExact=*, countPlural=one': 'one',
          'countPluralExact=*, countPlural=*': 'many',
        },
      },
    ]);
  });

  it('a plural with an offset', () => {
    expect(convert('{n, plural, offset:1 =1 {{who}} other {{who} and # more}}')).toEqual([
      {
        declarations: ['input n', 'input who', 'local nPluralExact = n'],
        selectors: ['nPluralExact'],
        match: {
          'nPluralExact=1': '{who}',
          'nPluralExact=*': '{who} and {n: icu:pound offset=1} more',
        },
      },
    ]);
    expect(convert('{n, plural, offset:1 one {a} other {b}}')).toMatchObject([
      { declarations: ['input n', 'local nPlural = n: plural offset=1'] },
    ]);
  });

  it('an ordinal', () => {
    expect(convert('{place, selectordinal, one {#st} two {#nd} few {#rd} other {#th}}')).toEqual([
      {
        declarations: ['input place', 'local placeOrdinal = place: plural type=ordinal'],
        selectors: ['placeOrdinal'],
        match: {
          'placeOrdinal=one': '{place: icu:pound}st',
          'placeOrdinal=two': '{place: icu:pound}nd',
          'placeOrdinal=few': '{place: icu:pound}rd',
          'placeOrdinal=*': '{place: icu:pound}th',
        },
      },
    ]);
  });

  it('a select', () => {
    expect(convert('{role, select, admin {All of it} viewer {Read it} other {Nothing}}')).toEqual([
      {
        declarations: ['input role'],
        selectors: ['role'],
        match: { 'role=admin': 'All of it', 'role=viewer': 'Read it', 'role=*': 'Nothing' },
      },
    ]);
  });

  it('a plural in each clause of a select, and text around them', () => {
    const icu =
      'Now {g, select, own {{n, plural, one {your card} other {your # cards}}} other {{n, plural, one {a card} other {# cards}}}}.';
    expect(convert(icu)).toEqual([
      {
        declarations: ['input g', 'input n', 'local nPlural = n: plural'],
        selectors: ['g', 'nPlural'],
        match: {
          'g=own, nPlural=one': 'Now your card.',
          'g=own, nPlural=*': 'Now your {n: icu:pound} cards.',
          'g=*, nPlural=one': 'Now a card.',
          'g=*, nPlural=*': 'Now {n: icu:pound} cards.',
        },
      },
    ]);
  });

  it('two selections next to each other, inside a tag', () => {
    expect(convert('<b>{a, select, x {1} other {2}}</b>{b, select, p {3} other {4}}')).toEqual([
      {
        declarations: ['input a', 'input b'],
        selectors: ['a', 'b'],
        match: {
          'a=x, b=p': '{#b}1{/b}3',
          'a=x, b=*': '{#b}1{/b}4',
          'a=*, b=p': '{#b}2{/b}3',
          'a=*, b=*': '{#b}2{/b}4',
        },
      },
    ]);
  });

  it.each([
    ['a date', '{d, date, short}', 'the date format of "d"'],
    ['a time', '{d, time}', 'the time format of "d"'],
    ['a number skeleton', '{n, number, ::currency/EUR}', 'the number style "::currency/EUR"'],
    ['an argument that is not a plain identifier', '{이름} 님', 'the argument name "이름"'],
    ['a tag with a hyphen', '<my-tag>x</my-tag>', 'the tag name "my-tag"'],
    [
      'an argument that selects twice in a row',
      '{n, plural, one {a} other {b}} {n, plural, one {c} other {d}}',
      'the argument "n" selects more than once',
    ],
    [
      'an argument that selects inside its own clause',
      '{g, select, a {{g, select, b {x} other {y}}} other {z}}',
      'the argument "g" selects more than once',
    ],
    [
      'an argument named like the local variable of a plural',
      '{nPlural} {n, plural, one {a} other {b}}',
      'the argument "nPlural" has the name of a local variable',
    ],
  ])('does not convert %s', (_, icu, reason) => {
    const result = convertMessage(icu);
    expect(result.ok).toBe(false);
    expect(result).toMatchObject({ reason: expect.stringContaining(reason) as unknown });
  });

  it('reports a text the original could not parse', () => {
    expect(convertMessage('Hello {name')).toEqual({
      ok: false,
      reason: 'ICU syntax: EXPECT_ARGUMENT_CLOSING_BRACE at offset 6',
    });
  });

  it('refuses a translation with an argument or a tag the English message lacks', () => {
    const english = shapeOf(parseIcu('Add <b>{username}</b> to board'));
    expect(convertMessage('<b>{username}</b> hinzufügen', english).ok).toBe(true);
    expect(convertMessage('Zum Board hinzufügen', english).ok).toBe(true);
    expect(convertMessage('<i>{benutzer}</i> hinzufügen', english)).toEqual({
      ok: false,
      reason: 'not in the English message: {benutzer}, <i>',
    });
  });
});

describe('the old names', () => {
  // Put together from halves, as in tools/names/check.ts.
  const product = ['Focal', 'board'].join('');
  const company = ['Matter', 'most'].join('');

  it('replaces the name of the product, whatever its case', () => {
    expect(renameProduct(`About ${product}`, 'en', 'x')).toBe('About fvoci');
    expect(renameProduct(`${product.toUpperCase()} and ${product.toLowerCase()}`, 'en', 'x')).toBe(
      'fvoci and fvoci',
    );
  });

  it('replaces the name of the company where it stands for the product', () => {
    const id = 'OnboardingTour.AddComments.Body';
    expect(renameProduct(`your fellow ${company} users`, 'en', id)).toBe('your fellow fvoci users');
    const korean = ['메터', '머스트'].join('');
    expect(renameProduct(`${korean} 사용자에게`, 'ko', id)).toBe('fvoci 사용자에게');
  });

  it('stops at the name of the company anywhere else', () => {
    expect(() => renameProduct(`Made by ${company}`, 'en', 'Sidebar.made-by')).toThrow(
      /names the old company/,
    );
  });
});

describe('converted messages, compiled by paraglide', () => {
  type Message = (inputs?: Record<string, unknown>, options?: { locale?: string }) => string;
  type Parts = (inputs?: Record<string, unknown>) => { type: string; value?: string }[];

  const english = {
    text: "It's '{'here'}', isn''t it \\",
    'GroupBy.hide-empty': 'Hide {count} empty groups',
    tag: 'Boards in <b>{name}</b> move',
    cards: '{count, plural, one {# card} other {# cards}}',
    exact: '{count, plural, =0 {No cards} one {One card} other {# cards}}',
    offset:
      '{count, plural, offset:1 =0 {Nobody} =1 {{who}} one {{who} and one other} other {{who} and # others}}',
    place: '{place, selectordinal, one {#st} two {#nd} few {#rd} other {#th}}',
    role: '{role, select, admin {All of it} viewer {Read it} other {Nothing}}',
    nested:
      '{g, select, own {{n, plural, one {your card} other {your # cards}}} other {{n, plural, one {a card} other {# cards}}}}',
    numbers: '{a, number} {b, number, integer} {c, number, percent}',
  };
  const russian = {
    cards:
      '{count, plural, one {# карточка} few {# карточки} many {# карточек} other {# карточки}}',
  };

  let directory = '';
  let m: Record<string, Message> = {};
  let parts: Parts = () => [];

  beforeAll(async () => {
    const cache = path.join(WEB_ROOT, 'node_modules/.cache');
    mkdirSync(cache, { recursive: true });
    directory = mkdtempSync(path.join(cache, 'i18n-test-'));
    mkdirSync(path.join(directory, 'project.inlang'));
    mkdirSync(path.join(directory, 'messages'));
    const plugin = path.join(WEB_ROOT, 'node_modules/@inlang/plugin-message-format/dist/index.js');
    writeFileSync(
      path.join(directory, 'project.inlang/settings.json'),
      JSON.stringify({
        baseLocale: 'en',
        locales: ['en', 'ru'],
        modules: [plugin],
        'plugin.inlang.messageFormat': { pathPattern: './messages/{locale}.json' },
      }),
    );
    for (const [locale, messages] of Object.entries({ en: english, ru: russian })) {
      const converted = Object.entries(messages).map(([id, icu]) => [id, convert(icu)]);
      writeFileSync(
        path.join(directory, `messages/${locale}.json`),
        JSON.stringify(Object.fromEntries(converted)),
      );
    }
    await compile({
      project: path.join(directory, 'project.inlang'),
      outdir: path.join(directory, 'out'),
      strategy: ['globalVariable', 'baseLocale'],
    });
    const compiled = (await import(
      pathToFileURL(path.join(directory, 'out/messages.js')).href
    )) as { m: Record<string, Message> & { tag: { parts: Parts } } };
    m = compiled.m;
    parts = compiled.m.tag.parts;
  });

  afterAll(() => {
    if (directory !== '') rmSync(directory, { recursive: true, force: true });
  });

  const call = (id: string, inputs?: Record<string, unknown>, locale?: string): string => {
    const message = m[id];
    if (!message) throw new Error(`no message ${id}`);
    return message(inputs, locale === undefined ? undefined : { locale });
  };

  it('text, with what ICU quoted', () => {
    expect(call('text')).toBe("It's {here}, isn't it \\");
  });

  it('an argument, in a message whose id has a dot and a hyphen', () => {
    expect(call('GroupBy.hide-empty', { count: 3 })).toBe('Hide 3 empty groups');
  });

  it('a tag: no markup in the text, and parts to render it from', () => {
    expect(call('tag', { name: 'Work' })).toBe('Boards in Work move');
    expect(parts({ name: 'Work' })).toMatchObject([
      { type: 'text', value: 'Boards in ' },
      { type: 'markup-start', name: 'b' },
      { type: 'text', value: 'Work' },
      { type: 'markup-end', name: 'b' },
      { type: 'text', value: ' move' },
    ]);
  });

  it.each([
    [0, '0 cards'],
    [1, '1 card'],
    [2, '2 cards'],
    [1000, '1,000 cards'],
  ])('a plural: %d', (count, text) => {
    expect(call('cards', { count })).toBe(text);
  });

  it.each([
    [1, '1 карточка'],
    [3, '3 карточки'],
    [5, '5 карточек'],
    [21, '21 карточка'],
    [1.5, '1,5 карточки'],
  ])('a plural with the categories of Russian: %d', (count, text) => {
    expect(call('cards', { count }, 'ru')).toBe(text);
  });

  it.each([
    [0, 'No cards'],
    [1, 'One card'],
    [2, '2 cards'],
  ])('an exact number before the category: %d', (count, text) => {
    expect(call('exact', { count })).toBe(text);
  });

  it.each([
    [0, 'Nobody'],
    [1, 'Ann'],
    [2, 'Ann and one other'],
    [3, 'Ann and 2 others'],
    [1001, 'Ann and 1,000 others'],
  ])('an offset: %d', (count, text) => {
    expect(call('offset', { count, who: 'Ann' })).toBe(text);
  });

  it.each([
    [1, '1st'],
    [2, '2nd'],
    [3, '3rd'],
    [4, '4th'],
    [11, '11th'],
    [22, '22nd'],
  ])('an ordinal: %d', (place, text) => {
    expect(call('place', { place })).toBe(text);
  });

  it.each([
    ['admin', 'All of it'],
    ['viewer', 'Read it'],
    ['guest', 'Nothing'],
  ])('a select: %s', (role, text) => {
    expect(call('role', { role })).toBe(text);
  });

  it.each([
    ['own', 1, 'your card'],
    ['own', 4, 'your 4 cards'],
    ['team', 1, 'a card'],
    ['team', 4, '4 cards'],
  ])('a plural inside a select: %s %d', (g, n, text) => {
    expect(call('nested', { g, n })).toBe(text);
  });

  it('numbers', () => {
    expect(call('numbers', { a: 1234.5, b: 3.7, c: 0.25 })).toBe('1,234.5 4 25%');
  });

  it('the English text where a locale has no translation', () => {
    expect(call('role', { role: 'admin' }, 'ru')).toBe('All of it');
  });
});

describe('the conversion of the original files', () => {
  const { files, report } = convertAll();
  const keys = (tag: string): string[] =>
    messageKeys(JSON.parse(files.get(`messages/${tag}.json`) ?? '{}'));

  it('converts the locales the original offers, and Korean', () => {
    expect(bundledFiles()).toHaveLength(17);
    expect(report.locales.map((locale) => locale.tag)).toEqual(LOCALES.map(({ tag }) => tag));
    expect(LOCALES.filter((locale) => locale.originalCode === undefined)).toMatchObject([
      { tag: 'ko' },
    ]);
  });

  it('gives Korean every key of English and no locale a key English lacks', () => {
    const english = keys('en');
    expect(compareKeys(english, keys('ko'))).toEqual({ missing: [], extra: [] });
    for (const { tag } of LOCALES) expect(compareKeys(english, keys(tag)).extra).toEqual([]);
  });

  it('keeps the messages only this app has, and the ids of the original as they are', () => {
    const english = keys('en');
    expect(english).toContain('ui_close');
    expect(english).toContain('BoardComponent.add-a-group');
    expect(english.indexOf('app_name')).toBe(0);
  });

  it('leaves out what is removed and what the original does not use', () => {
    const english = keys('en');
    expect(report.removed.length).toBeGreaterThan(0);
    expect(report.dead.length).toBeGreaterThan(0);
    for (const { id, usedIn } of report.removed) {
      expect(english).not.toContain(id);
      expect(usedIn.length).toBeGreaterThan(0);
    }
    for (const id of report.dead) expect(english).not.toContain(id);
  });

  it('writes the same files when run again', () => {
    expect([...convertAll().files]).toEqual([...files]);
  });
});
