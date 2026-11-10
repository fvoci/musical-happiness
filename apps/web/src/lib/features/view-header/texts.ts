/** The words and icons the parts of the header share. */
import CalendarDaysIcon from '@lucide/svelte/icons/calendar-days';
import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
import SquareKanbanIcon from '@lucide/svelte/icons/square-kanban';
import TableIcon from '@lucide/svelte/icons/table-2';
import type { Component } from 'svelte';

import { m } from '$lib/i18n/paraglide/messages.js';
import type { FilterConditionMessages } from '$lib/logic/filter-conditions.ts';
import {
  TITLE_COLUMN_ID,
  type BoardView,
  type PropertyTemplate,
  type ViewType,
} from '$lib/logic/wire.ts';

import { TITLE_FILTER_ID } from './filter-edit.ts';
import type { AddableViewType } from './view-fields.ts';

/** A Lucide icon. `data-icon` is a button's hint of where in it the icon sits. */
export type Icon = Component<{ class?: string; 'data-icon'?: string }>;

export const VIEW_ICONS: Record<ViewType, Icon> = {
  board: SquareKanbanIcon,
  table: TableIcon,
  gallery: LayoutGridIcon,
  calendar: CalendarDaysIcon,
};

/** The title of a view, or the words for one without a title. */
export function viewTitle(view: BoardView): string {
  return view.title || m['Sidebar.untitled-view']();
}

/** What a kind of view is called where one is added from a list of kinds. */
export const VIEW_KIND_NAMES: Record<AddableViewType, () => string> = {
  board: m['View.Board'],
  table: m['View.Table'],
  gallery: m['View.Gallery'],
};

/** The line that adds a view of a kind, where it stands among other lines. */
export const ADD_VIEW_LABELS: Record<AddableViewType, () => string> = {
  board: m.viewheader_add_board_view,
  table: m.viewheader_add_table_view,
  gallery: m.viewheader_add_gallery_view,
};

/** The title a new view of a kind starts with. */
export const NEW_VIEW_TITLES: Record<AddableViewType, () => string> = {
  board: m['View.NewBoardTitle'],
  table: m['View.NewTableTitle'],
  gallery: m['View.NewGalleryTitle'],
};

/** The names of the conditions of a filter. */
export const CONDITION_NAMES: FilterConditionMessages = {
  includes: m['Filter.includes'],
  notIncludes: m['Filter.not-includes'],
  isEmpty: m['Filter.is-empty'],
  isNotEmpty: m['Filter.is-not-empty'],
  isSet: m['Filter.is-set'],
  isNotSet: m['Filter.is-not-set'],
  is: m['Filter.is'],
  contains: m['Filter.contains'],
  notContains: m['Filter.not-contains'],
  startsWith: m['Filter.starts-with'],
  notStartsWith: m['Filter.not-starts-with'],
  endsWith: m['Filter.ends-with'],
  notEndsWith: m['Filter.not-ends-with'],
  isBefore: m['Filter.is-before'],
  isAfter: m['Filter.is-after'],
};

/**
 * The name of what a condition or a sort is on: a property of the board, or
 * the title of the card. An identifier nothing answers to is a property that
 * was deleted since.
 */
export function propertyName(propertyId: string, templates: readonly PropertyTemplate[]): string {
  if (propertyId === TITLE_FILTER_ID || propertyId === TITLE_COLUMN_ID) {
    return m['default-properties.title']();
  }
  return (
    templates.find((template) => template.id === propertyId)?.name ??
    m.viewheader_unknown_property()
  );
}
