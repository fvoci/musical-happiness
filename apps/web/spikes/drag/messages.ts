/** The texts of the spike. Plain English here; the product takes them from its messages. */
import type { SortableMessages, SortablePlace } from '$lib/drag/sortable.svelte.ts';

export const INSTRUCTIONS =
  'Drag a card with the mouse, or press and hold it with a finger. ' +
  'With the keyboard: Space picks up the focused card, the arrow keys move it, ' +
  'Space or Enter drops it, Escape cancels.';

const where = ({ position, count, list }: SortablePlace): string =>
  `position ${position} of ${count} in ${list}`;

export const MESSAGES: SortableMessages = {
  pickedUp: (place) => `Picked up ${place.item}, ${where(place)}.`,
  moved: (place) => `${place.item}, ${where(place)}.`,
  dropped: (place) => `Dropped ${place.item}, ${where(place)}.`,
  cancelled: (place) => `Cancelled. ${place.item} is back at ${where(place)}.`,
};
