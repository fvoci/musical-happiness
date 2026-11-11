/**
 * What the calendar writes to the board: a card's date moved to another
 * day. One change of one property, so that one undo takes it back
 * (`BoardStore.setCardProperty`).
 */
import { movedValue } from '$lib/logic/calendar.ts';
import type { CalendarDate } from '$lib/logic/dates.ts';
import {
  cardProperty,
  formatDateValue,
  parseDateValue,
  type Card,
  type PropertyTemplate,
} from '$lib/logic/wire.ts';
import type { BoardStore } from '$lib/state/index.ts';

/**
 * Moves the card's value of `template` (a date property) so that it starts
 * on `day`, keeping its length and time of day (`movedValue`); a card with no
 * value gets that day. True once the board has kept it: a change the backend
 * refuses is reported and put back by the board, and gives false.
 */
export async function moveCard(
  board: BoardStore,
  card: Card,
  template: PropertyTemplate,
  day: CalendarDate,
  timeZone: string,
): Promise<boolean> {
  const value = parseDateValue(cardProperty(card, template.id));
  const next = formatDateValue(movedValue(value, day, timeZone));
  await board.setCardProperty(card.id, template.id, next);
  const now = board.card(card.id);
  if (!now) return false;
  // Both read the same way, whatever order the backend gave the ends in.
  const kept = formatDateValue(parseDateValue(cardProperty(now, template.id)));
  return kept?.from === next?.from && kept?.to === next?.to;
}
