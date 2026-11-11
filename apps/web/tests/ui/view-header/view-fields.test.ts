/**
 * What the header writes into a view, and the changes to its sort: the
 * fields of a new view, of a change of grouping, the list of properties a
 * view can show.
 */
import { describe, expect, it } from 'vitest';

import {
  addSort,
  moveSort,
  removeSort,
  reverseSort,
  sortCandidates,
} from '$lib/features/view-header/sort-edit.ts';
import {
  groupableProperties,
  groupingFields,
  groupingOf,
  groupProperty,
  newViewFields,
  propertyToggles,
  sortOffered,
  withPropertyVisible,
} from '$lib/features/view-header/view-fields.ts';
import {
  createBoardView,
  TITLE_COLUMN_ID,
  type BoardView,
  type PropertyTemplate,
  type SortOption,
} from '$lib/logic/wire.ts';

const TEMPLATES: PropertyTemplate[] = [
  { id: 'hours', name: 'Hours', type: 'number', options: [] },
  {
    id: 'status',
    name: 'Status',
    type: 'select',
    options: [
      { id: 'todo', value: 'To do', color: 'propColorBlue' },
      { id: 'done', value: 'Done', color: 'propColorGreen' },
    ],
  },
  { id: 'tags', name: 'Tags', type: 'multiSelect', options: [] },
  { id: 'owner', name: 'Owner', type: 'person', options: [] },
  { id: 'made', name: 'Made by', type: 'createdBy', options: [] },
];

function viewOf(fields: Partial<BoardView['fields']>): BoardView {
  const view = createBoardView();
  return { ...view, fields: { ...view.fields, ...fields } };
}

describe('a new view', () => {
  it('as a board is grouped by the first select property, every column showing', () => {
    const fields = newViewFields('board', TEMPLATES);
    expect(fields.viewType).toBe('board');
    expect(fields.groupById).toBe('status');
    expect(fields.visibleOptionIds).toEqual(['', 'todo', 'done']);
    expect(fields.hiddenOptionIds).toEqual([]);
    expect(fields.visiblePropertyIds).toEqual([]);
  });

  it('as a board on a board without a select property is grouped by nothing yet', () => {
    const fields = newViewFields('board', TEMPLATES.slice(0, 1));
    expect(fields.groupById).toBeUndefined();
    expect(fields.visibleOptionIds).toEqual([]);
  });

  it('as a table shows every property, ungrouped, and leaves the width of the title column to the table', () => {
    const fields = newViewFields('table', TEMPLATES);
    expect(fields.viewType).toBe('table');
    expect(fields.visiblePropertyIds).toEqual(['hours', 'status', 'tags', 'owner', 'made']);
    expect(fields.groupById).toBe('');
    expect(fields.columnWidths).toEqual({});
  });

  it('as a gallery shows the titles', () => {
    const fields = newViewFields('gallery', TEMPLATES);
    expect(fields.viewType).toBe('gallery');
    expect(fields.visiblePropertyIds).toEqual([TITLE_COLUMN_ID]);
  });

  it('has every member a view has, none shared with another new view', () => {
    const first = newViewFields('table', TEMPLATES);
    const second = newViewFields('table', TEMPLATES);
    expect(Object.keys(first).sort()).toEqual(
      Object.keys({ ...createBoardView().fields, groupById: '' }).sort(),
    );
    expect(first.filter).toEqual({ operation: 'and', filters: [] });
    expect(first.filter).not.toBe(second.filter);
    expect(first.sortOptions).toEqual([]);
  });
});

describe('grouping', () => {
  it('is offered for boards and tables, and only a table can do without', () => {
    expect(groupingOf('board')).toEqual({ offered: true, optional: false });
    expect(groupingOf('table')).toEqual({ offered: true, optional: true });
    expect(groupingOf('gallery').offered).toBe(false);
    expect(groupingOf('calendar').offered).toBe(false);
  });

  it('can be by a select property, a person, or who made or changed the card', () => {
    expect(groupableProperties(TEMPLATES).map((template) => template.id)).toEqual([
      'status',
      'owner',
      'made',
    ]);
  });

  it('starts the columns over: no value first, then the options, nothing hidden or collapsed', () => {
    expect(groupingFields(TEMPLATES[1])).toEqual({
      groupById: 'status',
      visibleOptionIds: ['', 'todo', 'done'],
      hiddenOptionIds: [],
      collapsedOptionIds: [],
    });
    // A person has no options: the columns are the people the cards have.
    expect(groupingFields(TEMPLATES[3]).visibleOptionIds).toEqual(['']);
  });

  it('by nothing is the empty identifier and no columns', () => {
    expect(groupingFields(undefined)).toEqual({
      groupById: '',
      visibleOptionIds: [],
      hiddenOptionIds: [],
      collapsedOptionIds: [],
    });
  });

  it('of a view is the property it names; a board that names none uses the first select', () => {
    expect(groupProperty(viewOf({ viewType: 'board', groupById: 'owner' }), TEMPLATES)?.id).toBe(
      'owner',
    );
    expect(groupProperty(viewOf({ viewType: 'board' }), TEMPLATES)?.id).toBe('status');
    expect(groupProperty(viewOf({ viewType: 'board', groupById: 'gone' }), TEMPLATES)?.id).toBe(
      'status',
    );
    expect(groupProperty(viewOf({ viewType: 'table', groupById: '' }), TEMPLATES)).toBeUndefined();
    expect(groupProperty(viewOf({ viewType: 'table' }), TEMPLATES)).toBeUndefined();
  });
});

describe('the properties a view shows', () => {
  it('can be every property of the board, and nothing a view does not read', () => {
    expect(propertyToggles(TEMPLATES).map((toggle) => toggle.id)).toEqual(
      TEMPLATES.map((template) => template.id),
    );
  });

  it('include a card relation in a table only: the face of a card does not show it', () => {
    const withRelation: PropertyTemplate[] = [
      ...TEMPLATES,
      { id: 'after', name: 'After', type: 'relation', options: [] },
    ];
    expect(propertyToggles(withRelation, 'table').map((toggle) => toggle.id)).toContain('after');
    for (const viewType of ['board', 'gallery', 'calendar'] as const) {
      expect(propertyToggles(withRelation, viewType).map((toggle) => toggle.id)).toEqual(
        TEMPLATES.map((template) => template.id),
      );
    }
  });

  it('get a newly shown one at the end, and lose a hidden one where it was', () => {
    expect(withPropertyVisible(['a', 'b'], 'c', true)).toEqual(['a', 'b', 'c']);
    expect(withPropertyVisible(['a', 'b', 'c'], 'b', false)).toEqual(['a', 'c']);
    expect(withPropertyVisible(['a', 'b'], 'a', true)).toEqual(['a', 'b']);
    expect(withPropertyVisible(['a', 'b'], 'z', false)).toEqual(['a', 'b']);
  });
});

describe('the sort of a view', () => {
  const options: readonly SortOption[] = [
    { propertyId: 'status', reversed: false },
    { propertyId: TITLE_COLUMN_ID, reversed: true },
  ];

  it('is not offered for a calendar', () => {
    expect(sortOffered('calendar')).toBe(false);
    expect(['board', 'table', 'gallery'].map((type) => sortOffered(type as never))).toEqual([
      true,
      true,
      true,
    ]);
  });

  it('can be added on the title and on every property, each once', () => {
    expect(sortCandidates([], TEMPLATES)).toEqual([
      TITLE_COLUMN_ID,
      'hours',
      'status',
      'tags',
      'owner',
      'made',
    ]);
    expect(sortCandidates(options, TEMPLATES)).toEqual(['hours', 'tags', 'owner', 'made']);
    expect(addSort(options, 'status')).toBe(options);
  });

  it('gets a new line at the end, ascending', () => {
    expect(addSort(options, 'hours')).toEqual([
      ...options,
      { propertyId: 'hours', reversed: false },
    ]);
  });

  it('reverses, removes and moves a line', () => {
    expect(reverseSort(options, 1)).toEqual([
      options[0],
      { propertyId: TITLE_COLUMN_ID, reversed: false },
    ]);
    expect(removeSort(options, 0)).toEqual([options[1]]);
    expect(moveSort(options, 1, -1)).toEqual([options[1], options[0]]);
    expect(moveSort(options, 0, 1)).toEqual([options[1], options[0]]);
  });

  it('returns the list it was given when there is nothing to do', () => {
    expect(moveSort(options, 0, -1)).toBe(options);
    expect(moveSort(options, 1, 1)).toBe(options);
    expect(reverseSort(options, 5)).toBe(options);
    expect(removeSort(options, 5)).toBe(options);
  });
});
