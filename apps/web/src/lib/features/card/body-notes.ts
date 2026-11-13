/**
 * What the body of a card says about its room: the line under it while it
 * cannot be written (and why), and the notice when changes of the reader
 * were dropped with the document they were in.
 */
import { m } from '$lib/i18n/paraglide/messages.js';
import type { BodyDocState } from '$lib/state/backend.ts';

export interface RoomNote {
  text: string;
  /** The reader can ask to be let in again now. */
  retry: boolean;
  /** Said only when it lasts: joining and receiving are usually over at once. */
  late: boolean;
}

/** The line for a room; none when the reader may write. */
export function roomNote(room: BodyDocState, canEdit: boolean): RoomNote | undefined {
  const note = (text: string, more: Partial<RoomNote> = {}): RoomNote => ({
    text,
    retry: false,
    late: false,
    ...more,
  });
  switch (room.status) {
    case 'joining':
      return note(m.card_body_joining(), { late: true });
    case 'receiving':
      return note(m.card_body_receiving(), { late: true });
    case 'offline':
      return note(m.card_body_offline());
    case 'out':
      if (room.reason === 'not_found') return note(m.card_body_gone());
      return note(m.card_body_unavailable(), { retry: true });
    case 'joined':
      if (room.mode === 'read' || !canEdit) return note(m.card_body_read_only());
      return undefined;
  }
}

/** The notice for changes dropped, by the reason the room gave. */
export function lostNote(why: string): string {
  if (why === 'document_reset') return m.card_body_lost_reset();
  if (why === 'update_too_large' || why === 'document_too_large') return m.card_body_lost_large();
  return m.card_body_lost();
}
