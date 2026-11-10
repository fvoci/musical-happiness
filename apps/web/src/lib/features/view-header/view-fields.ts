/**
 * What the header writes into a view: the fields of a new view, of a change
 * of grouping, and of the properties a view shows.
 *
 * Everything here is a pure function of the view and the board's property
 * definitions, so that the choices made (what a new view starts with, what a
 * change of grouping resets) are in one place and are tested without a
 * screen.
 */
import { propertyBehavior } from '$lib/logic/properties.ts';
import {
  createBoardView,
  TITLE_COLUMN_ID,
  type BoardView,
  type PropertyTemplate,
  type ViewType,
} from '$lib/logic/wire.ts';

type ViewFields = BoardView['fields'];

/** The pseudo property that stands for the marks of comments and description on a card. */
export const BADGES_COLUMN_ID = '__badges';

/** The width a new table gives its title column, in pixels. */
export const DEFAULT_TITLE_COLUMN_WIDTH = 280;

/** The kinds of view that can be added. A calendar cannot, until there is a calendar view. */
export const ADDABLE_VIEW_TYPES = ['board', 'table', 'gallery'] as const;
export type AddableViewType = (typeof ADDABLE_VIEW_TYPES)[number];

/** Whether a view of this kind has a grouping, and whether "no grouping" is a choice. */
export function groupingOf(viewType: ViewType): { offered: boolean; optional: boolean } {
  return { offered: viewType === 'board' || viewType === 'table', optional: viewType === 'table' };
}

/** A calendar places its cards by date: it has no sort. */
export function sortOffered(viewType: ViewType): boolean {
  return viewType !== 'calendar';
}

/** The properties a view can be grouped by. */
export function groupableProperties(templates: readonly PropertyTemplate[]): PropertyTemplate[] {
  return templates.filter((template) => propertyBehavior(template.type).canGroup);
}

/**
 * The property a view is grouped by. A board view that names none is grouped
 * by the first select property, as it is drawn.
 */
export function groupProperty(
  view: BoardView,
  templates: readonly PropertyTemplate[],
): PropertyTemplate | undefined {
  const named = templates.find((template) => template.id === view.fields.groupById);
  if (named || view.fields.viewType !== 'board') return named;
  return templates.find((template) => template.type === 'select');
}

/**
 * The fields of a view grouped by `template`, or by nothing. The columns
 * start over with the new property: the one of cards without a value first,
 * then one per option, none hidden and none collapsed. What was hidden or
 * collapsed belonged to the options of the old property.
 */
export function groupingFields(
  template: PropertyTemplate | undefined,
): Pick<ViewFields, 'groupById' | 'visibleOptionIds' | 'hiddenOptionIds' | 'collapsedOptionIds'> {
  return {
    // Not grouped is the empty identifier, as the built-in templates write it.
    groupById: template?.id ?? '',
    visibleOptionIds: template ? ['', ...template.options.map((option) => option.id)] : [],
    hiddenOptionIds: [],
    collapsedOptionIds: [],
  };
}

/**
 * The fields a new view starts with: a board grouped by the first select
 * property, a table that shows every property, a gallery that shows titles.
 */
export function newViewFields(
  viewType: AddableViewType,
  templates: readonly PropertyTemplate[],
): ViewFields {
  const fields: ViewFields = { ...createBoardView().fields, viewType };
  if (viewType === 'board') {
    const first = templates.find((template) => template.type === 'select');
    return first ? { ...fields, ...groupingFields(first) } : fields;
  }
  if (viewType === 'table') {
    return {
      ...fields,
      ...groupingFields(undefined),
      visiblePropertyIds: templates.map((template) => template.id),
      columnWidths: { [TITLE_COLUMN_ID]: DEFAULT_TITLE_COLUMN_WIDTH },
    };
  }
  return { ...fields, visiblePropertyIds: [TITLE_COLUMN_ID] };
}

/** One line of the list of what a view shows on its cards. */
export interface PropertyToggle {
  id: string;
  /** `title` and `badges` are not properties of the board; the screen has their names. */
  kind: 'title' | 'property' | 'badges';
  template?: PropertyTemplate;
}

/**
 * What can be shown or hidden in a view of this kind, in the order of the
 * list. A gallery can hide its titles; a board, a gallery and a calendar can
 * show the marks of comments and description. A table always has its title
 * column and has no marks.
 */
export function propertyToggles(
  viewType: ViewType,
  templates: readonly PropertyTemplate[],
): PropertyToggle[] {
  const toggles: PropertyToggle[] = templates.map((template) => ({
    id: template.id,
    kind: 'property',
    template,
  }));
  if (viewType === 'gallery') toggles.unshift({ id: TITLE_COLUMN_ID, kind: 'title' });
  if (viewType !== 'table') toggles.push({ id: BADGES_COLUMN_ID, kind: 'badges' });
  return toggles;
}

/** The visible properties with `propertyId` shown or hidden. A newly shown one goes last. */
export function withPropertyVisible(
  visiblePropertyIds: readonly string[],
  propertyId: string,
  visible: boolean,
): string[] {
  const others = visiblePropertyIds.filter((id) => id !== propertyId);
  if (!visible) return others;
  return visiblePropertyIds.includes(propertyId)
    ? [...visiblePropertyIds]
    : [...others, propertyId];
}
