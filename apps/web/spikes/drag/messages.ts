/** The texts of the spike. Plain English here; the product takes them from its messages. */
import type {
  SortableMessages,
  SortablePlace,
  SortableZonePlace,
} from '$lib/drag/sortable.svelte.ts';

/**
 * What can be tried on this page, shown at its top. Each line is short enough
 * to stay one line on a phone 360 pixels wide.
 */
export const THINGS_TO_TRY = [
  'Drag a card: mouse, or press and hold.',
  'Drag a column by its heading.',
  'A finger that lifts a card buzzes (Android).',
  'Let go outside the board, or Escape: cancel.',
  'Sorted view: you pick only the column.',
  'Select cards: tap several, drag one.',
  'Hidden column: a drop target at the far end.',
  'Keys: Space lifts, arrows move, Space drops.',
];

/** Read to a screen reader with every card. */
export const INSTRUCTIONS =
  'Drag a card with the mouse, or press and hold it with a finger. ' +
  'With the keyboard: Space picks up the focused card, the arrow keys move it, ' +
  'Space or Enter drops it, Escape cancels.';

/** Read to a screen reader with every column heading. */
export const COLUMN_INSTRUCTIONS =
  'Drag the heading to move the column. ' +
  'With the keyboard: Space picks up the column, the left and right arrow keys move it, ' +
  'Space or Enter drops it, Escape cancels.';

const what = ({ item, items }: SortablePlace | SortableZonePlace): string =>
  items > 1 ? `${item} and ${items - 1} more` : item;

const where = ({ position, count, list }: SortablePlace): string =>
  `position ${position} of ${count} in ${list}`;

export const MESSAGES: SortableMessages = {
  pickedUp: (place) => `Picked up ${what(place)}, ${where(place)}.`,
  moved: (place) => `${what(place)}, ${where(place)}.`,
  dropped: (place) => `Dropped ${what(place)}, ${where(place)}.`,
  cancelled: (place) => `Cancelled. ${what(place)} is back at ${where(place)}.`,
  sorted: () => 'This view is sorted: choose a column, the order decides the place.',
  outside: () => 'Let go to cancel.',
  movedToZone: (place) => `${what(place)}, over ${place.zone}.`,
  droppedInZone: (place) => `Dropped ${what(place)} on ${place.zone}.`,
};

/** For the columns: one row, so a place is a position in it. */
export const COLUMN_MESSAGES: SortableMessages = {
  pickedUp: (place) => `Picked up column ${place.item}, ${place.position} of ${place.count}.`,
  moved: (place) => `Column ${place.item}, ${place.position} of ${place.count}.`,
  dropped: (place) => `Dropped column ${place.item}, ${place.position} of ${place.count}.`,
  cancelled: (place) =>
    `Cancelled. Column ${place.item} is back at ${place.position} of ${place.count}.`,
  outside: () => 'Let go to cancel.',
};
