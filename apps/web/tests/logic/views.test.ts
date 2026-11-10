/**
 * Search, grouping, the property table and CSV rows. The original has no
 * tests for this code; these were written for the port. Cases named
 * "quirk Qn" pin a behaviour of the original that looks like a defect and
 * is listed in PORTING.md.
 */
import { describe, expect, it, vi } from 'vitest';

import { mergeBlocks } from '$lib/logic/block-tree.ts';
import { csvText, tableRows } from '$lib/logic/csv.ts';
import { diagnostics } from '$lib/logic/diagnostics.ts';
import {
  filterConditionDisplayString,
  type FilterConditionMessages,
} from '$lib/logic/filter-conditions.ts';
import { getVisibleAndHiddenGroups } from '$lib/logic/group.ts';
import {
  HASH_SIGN_TOKEN,
  PROPERTY_BEHAVIORS,
  propertyBehavior,
  SELECTABLE_PROPERTY_TYPES,
} from '$lib/logic/properties.ts';
import { searchFilterCards } from '$lib/logic/search.ts';
import {
  createBlock,
  createCard,
  setCardProperty,
  type Card,
  type PropertyTemplate,
  type PropertyValue,
} from '$lib/logic/wire.ts';

function card(id: string, title: string, properties: Record<string, PropertyValue> = {}): Card {
  const created = createCard();
  created.id = id;
  created.title = title;
  for (const [propertyId, value] of Object.entries(properties)) {
    setCardProperty(created, propertyId, value);
  }
  return created;
}

const ids = (cards: readonly Card[]): string => cards.map((item) => item.id).join(' ');

const status: PropertyTemplate = {
  id: 'status',
  name: 'Status',
  type: 'select',
  options: [
    { id: 'todo', value: 'To do', color: '' },
    { id: 'doing', value: 'In progress', color: '' },
    { id: 'done', value: 'Done', color: '' },
  ],
};
const tags: PropertyTemplate = {
  id: 'tags',
  name: 'Tags',
  type: 'multiSelect',
  options: [
    { id: 't1', value: 'Urgent', color: '' },
    { id: 't2', value: 'Backend work', color: '' },
  ],
};
const notes: PropertyTemplate = { id: 'notes', name: 'Notes', type: 'text', options: [] };
const due: PropertyTemplate = { id: 'due', name: 'Due', type: 'date', options: [] };
const amount: PropertyTemplate = { id: 'amount', name: 'Amount', type: 'number', options: [] };
const owner: PropertyTemplate = { id: 'owner', name: 'Owner', type: 'person', options: [] };
const context = { locale: 'en-US', now: new Date(2026, 0, 15) };

describe('search', () => {
  const templates = [status, tags, notes, due];
  const cards = [
    card('title', 'Fix the Login page'),
    card('select', 'a', { status: 'doing' }),
    card('text', 'b', { notes: 'ask about LOGIN flow' }),
    card('multi', 'c', { tags: ['t1', 't2'] }),
    card('date', 'd', { due: '{"from":1767225600000}' }),
  ];

  it('returns every card for an empty text, as a new list', () => {
    const result = searchFilterCards(cards, templates, '');
    expect(result).toEqual(cards);
    expect(result).not.toBe(cards);
  });

  it('finds the text in titles and text values, ignoring case', () => {
    expect(ids(searchFilterCards(cards, templates, 'login'))).toBe('title text');
  });

  it('finds a select by a part of the label of its option', () => {
    expect(ids(searchFilterCards(cards, templates, 'progress'))).toBe('select');
  });

  it('does not search date values', () => {
    expect(ids(searchFilterCards(cards, templates, '1767'))).toBe('');
  });

  it('skips a property whose stored value is null', () => {
    const odd = card('odd', 'x');
    setCardProperty(odd, 'notes', null as unknown as string);
    expect(ids(searchFilterCards([odd], templates, 'anything'))).toBe('');
  });

  it('quirk Q3: a multi-select only matches a whole label', () => {
    expect(ids(searchFilterCards(cards, templates, 'urgent'))).toBe('multi');
    expect(ids(searchFilterCards(cards, templates, 'backend'))).toBe('');
    expect(ids(searchFilterCards(cards, templates, 'backend work'))).toBe('multi');
  });
});

describe('groups of a board view', () => {
  const noValue = (name: string): string => `No ${name}`;
  const cards = [
    card('a', 'a', { status: 'todo' }),
    card('b', 'b', { status: 'done' }),
    card('c', 'c'),
    card('d', 'd', { status: 'deleted-option' }),
  ];

  it('puts the cards without a value first, then the options in the order of the view', () => {
    const groups = getVisibleAndHiddenGroups(cards, ['done', 'todo'], [], status, noValue);
    expect(groups.visible.map((group) => [group.option.value, ids(group.cards)])).toEqual([
      ['No Status', 'c d'],
      ['Done', 'b'],
      ['To do', 'a'],
      // An option the view does not mention is shown after the others.
      ['In progress', ''],
    ]);
    expect(groups.hidden).toEqual([]);
  });

  it('keeps hidden options apart and honours a placed empty group', () => {
    const groups = getVisibleAndHiddenGroups(cards, ['todo', ''], ['done'], status, noValue);
    expect(groups.visible.map((group) => group.option.id)).toEqual(['todo', '', 'doing']);
    expect(groups.hidden.map((group) => [group.option.id, ids(group.cards)])).toEqual([
      ['done', 'b'],
    ]);
  });

  it('ignores the identifier of an option that was deleted', () => {
    const groups = getVisibleAndHiddenGroups(cards, ['gone', 'todo'], [], status, noValue);
    expect(groups.visible.map((group) => group.option.id)).toEqual(['', 'todo', 'doing', 'done']);
  });

  it('groups by person in the order people first occur', () => {
    const people = [
      card('1', '1', { owner: 'ann' }),
      card('2', '2', { owner: 'bob' }),
      card('3', '3', { owner: 'ann' }),
    ];
    const groups = getVisibleAndHiddenGroups(people, [], ['bob'], owner, noValue);
    expect(groups.visible.map((group) => [group.option.id, ids(group.cards)])).toEqual([
      ['ann', '1 3'],
    ]);
    expect(groups.hidden.map((group) => [group.option.id, ids(group.cards)])).toEqual([
      ['bob', '2'],
    ]);
  });

  it('never hides the group of cards that have no creator recorded', () => {
    const by: PropertyTemplate = { id: 'by', name: 'By', type: 'createdBy', options: [] };
    const groups = getVisibleAndHiddenGroups([card('1', '1')], [], [''], by, noValue);
    expect(groups.visible.map((group) => group.option.id)).toEqual(['']);
    expect(groups.hidden).toEqual([]);
  });

  it('quirk Q4: cards without a person form a group called "undefined"', () => {
    const groups = getVisibleAndHiddenGroups([card('1', '1')], [], [], owner, noValue);
    expect(groups.visible.map((group) => group.option)).toEqual([
      { id: 'undefined', value: 'undefined', color: '' },
    ]);
  });
});

describe('property behaviour', () => {
  it('knows every kind a person can choose, and the two that are not offered', () => {
    expect(Object.keys(PROPERTY_BEHAVIORS).sort()).toEqual(
      [...SELECTABLE_PROPERTY_TYPES, 'file', 'unknown'].sort(),
    );
    expect(propertyBehavior('something-new')).toBe(PROPERTY_BEHAVIORS.unknown);
  });

  it('says which kinds can group, filter and be edited', () => {
    const where = (test: (name: string) => boolean): string[] =>
      Object.keys(PROPERTY_BEHAVIORS).filter(test).sort();
    expect(where((name) => propertyBehavior(name).canGroup)).toEqual([
      'createdBy',
      'person',
      'select',
      'updatedBy',
    ]);
    expect(where((name) => !propertyBehavior(name).canFilter)).toEqual([
      'file',
      'number',
      'unknown',
    ]);
    expect(where((name) => propertyBehavior(name).isReadOnly)).toEqual([
      'createdBy',
      'createdTime',
      'updatedBy',
      'updatedTime',
    ]);
    expect(PROPERTY_BEHAVIORS.checkbox.filterValueType).toBe('boolean');
    expect(PROPERTY_BEHAVIORS.multiPerson.filterValueType).toBe('person');
    expect(PROPERTY_BEHAVIORS.createdTime.filterValueType).toBe('date');
  });

  it('offers number calculations for numbers and time calculations for the times of a card', () => {
    expect(PROPERTY_BEHAVIORS.number.calculationOptions).toContain('median');
    expect(PROPERTY_BEHAVIORS.text.calculationOptions).not.toContain('sum');
    expect(PROPERTY_BEHAVIORS.createdTime.calculationOptions).toContain('dateRange');
    // The date property itself does not offer earliest and latest in the original.
    expect(PROPERTY_BEHAVIORS.date.calculationOptions).not.toContain('earliest');
  });

  it('shows a select by the label of its option', () => {
    const report = vi.spyOn(diagnostics, 'report').mockImplementation(() => undefined);
    const { displayValue } = PROPERTY_BEHAVIORS.select;
    expect(displayValue('done', card('x', 'x'), status, context)).toBe('Done');
    expect(displayValue(undefined, card('x', 'x'), status, context)).toBe('');
    expect(displayValue('gone', card('x', 'x'), status, context)).toBe('(Unknown)');
    expect(
      displayValue('gone', card('x', 'x'), status, { ...context, unknownOption: '(없음)' }),
    ).toBe('(없음)');
    expect(report).toHaveBeenCalledTimes(2);
    report.mockRestore();
  });

  it('shows a multi-select as the labels of its options, in the order of the board', () => {
    const { displayValue } = PROPERTY_BEHAVIORS.multiSelect;
    expect(displayValue(['t2', 't1'], card('x', 'x'), tags, context)).toEqual([
      'Urgent',
      'Backend work',
    ]);
    expect(displayValue([], card('x', 'x'), tags, context)).toBe('');
  });

  it('shows a date and a range of dates', () => {
    const { displayValue } = PROPERTY_BEHAVIORS.date;
    const day = (month: number, date: number): number => new Date(2026, month, date, 12).getTime();
    expect(displayValue(`{"from":${day(2, 5)}}`, card('x', 'x'), due, context)).toBe('March 05');
    expect(
      displayValue(`{"from":${day(2, 5)},"to":${day(2, 9)}}`, card('x', 'x'), due, context),
    ).toBe('March 05 -> March 09');
    expect(displayValue(String(day(2, 5)), card('x', 'x'), due, context)).toBe('March 05');
    expect(displayValue('not a date', card('x', 'x'), due, context)).toBe('');
    expect(displayValue(undefined, card('x', 'x'), due, context)).toBe('');
  });

  it('shows the times of the card itself', () => {
    const created = card('x', 'x');
    created.createAt = new Date(2026, 2, 5, 15, 20).getTime();
    expect(PROPERTY_BEHAVIORS.createdTime.displayValue(undefined, created, due, context)).toBe(
      'March 05 at 3:20 PM',
    );
    expect(PROPERTY_BEHAVIORS.createdTime.dateRange?.(undefined, created).from?.getTime()).toBe(
      created.createAt,
    );
  });
});

describe('CSV rows', () => {
  const templates = [status, tags, notes, amount, due];
  const view = {
    viewType: 'table' as const,
    visiblePropertyIds: ['notes', 'status', 'tags', 'amount'],
    dateDisplayPropertyId: undefined,
  };
  const first = card('1', 'Say "hi" #1', {
    status: 'done',
    tags: ['t1', 't2'],
    notes: 'a, b',
    amount: '012.50',
  });

  it('writes a header and a row per card, properties in the order of the board', () => {
    const rows = tableRows([first, card('2', 'Empty')], templates, view, 'Name', context);
    expect(rows).toEqual([
      ['Name', 'Status', 'Tags', 'Notes', 'Amount'],
      [`"Say ""hi"" ${HASH_SIGN_TOKEN}1"`, '"Done"', 'Urgent|Backend work', '"a, b"', '12.5'],
      ['"Empty"', '""', '', '', ''],
    ]);
    expect(csvText(rows).split('\r\n')).toHaveLength(4);
  });

  it('adds the date a calendar view places its cards by', () => {
    const calendar = { ...view, viewType: 'calendar' as const, dateDisplayPropertyId: 'due' };
    expect(tableRows([], templates, calendar, 'Name', context)[0]).toEqual([
      'Name',
      'Status',
      'Tags',
      'Notes',
      'Amount',
      'Due',
    ]);
    expect(
      tableRows([], templates, { ...view, dateDisplayPropertyId: 'due' }, 'Name', context)[0],
    ).toHaveLength(5);
  });

  it('exports who created a card from the card itself', () => {
    const by: PropertyTemplate = { id: 'by', name: 'By', type: 'createdBy', options: [] };
    const created = card('1', 't');
    created.createdBy = 'user-1';
    const rows = tableRows(
      [created],
      [by],
      { ...view, visiblePropertyIds: ['by'] },
      'Name',
      context,
    );
    expect(rows[1]).toEqual(['"t"', '"user-1"']);
  });

  it('quirk Q12: header cells are not quoted', () => {
    const comma: PropertyTemplate = { id: 'c', name: 'Cost, total', type: 'text', options: [] };
    const rows = tableRows([], [comma], { ...view, visiblePropertyIds: ['c'] }, 'Name', context);
    expect(csvText(rows)).toBe('Name,Cost, total\r\n');
  });
});

describe('merging updated blocks', () => {
  it('replaces updated blocks, appends new ones and drops deleted ones', () => {
    const block = (id: string, title: string, deleteAt = 0) => ({
      ...createBlock(),
      id,
      title,
      deleteAt,
    });
    const merged = mergeBlocks(
      [block('a', 'old a'), block('b', 'old b'), block('c', 'old c')],
      [block('b', 'new b'), block('c', 'gone', 5), block('d', 'new d')],
    );
    expect(merged.map((item) => `${item.id}:${item.title}`)).toEqual([
      'a:old a',
      'b:new b',
      'd:new d',
    ]);
  });
});

describe('names of filter conditions', () => {
  const messages = new Proxy({} as FilterConditionMessages, {
    get: (_target, name) => () => `<${String(name)}>`,
  });

  it('names a condition the kind of value offers', () => {
    expect(filterConditionDisplayString('notIncludes', messages, 'options')).toBe('<notIncludes>');
    expect(filterConditionDisplayString('isEmpty', messages, 'person')).toBe('<isEmpty>');
    expect(filterConditionDisplayString('isAfter', messages, 'date')).toBe('<isAfter>');
    expect(filterConditionDisplayString('endsWith', messages, 'text')).toBe('<endsWith>');
  });

  it('shows the default of the kind for a condition it does not offer', () => {
    expect(filterConditionDisplayString('is', messages, 'options')).toBe('<includes>');
    expect(filterConditionDisplayString('includes', messages, 'boolean')).toBe('<isSet>');
    expect(filterConditionDisplayString('includes', messages, 'date')).toBe('<is>');
    expect(filterConditionDisplayString('includes', messages, 'none')).toBe('(unknown)');
  });
});
