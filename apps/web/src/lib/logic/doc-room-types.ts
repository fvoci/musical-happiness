/**
 * What a room of a card's body says and hears (`contracts/websocket.md`,
 * section 6), as the room's state machine (`doc-room.ts`) and the ways
 * into a room (the page's connection, the rooms of the backend in memory)
 * share it.
 */

export type DocMode = 'write' | 'read';

/** A sync message of a room: `doc-frame.ts` reads and writes its bytes. */
export interface RoomFrame {
  doc: number;
  epoch: number;
  /** 0: step 1, 1: step 2, 2: update. */
  sync: number;
  payload: Uint8Array;
}

/** What the server says of a room; a join's two answers come without a room number that is known. */
export type RoomMessage =
  | { action: 'DOC_JOINED'; doc: number; epoch: number; mode: DocMode }
  | { action: 'DOC_REFUSED'; reason: string }
  | { action: 'DOC_ACK'; doc: number; epoch: number }
  | { action: 'DOC_NACK'; doc: number; epoch: number; reason: string }
  | { action: 'DOC_MODE'; doc: number; mode: DocMode }
  | { action: 'DOC_RESET'; doc: number; epoch: number }
  | { action: 'DOC_LEFT'; doc: number; reason: string };

/** What a room is told. */
export interface RoomHears {
  /** The connection is there: the room may be joined. */
  up(): void;
  /** The connection is gone, and every room with it. */
  down(): void;
  message(message: RoomMessage): void;
  frame(frame: RoomFrame): void;
}

/** What a room says. */
export interface RoomPort {
  /** The connection is there now: the room may be joined (`up` is told only of later ones). */
  readonly connected: boolean;
  /** Sends `JOIN_DOC` for the card. */
  join(): void;
  /** Sends a binary frame. */
  send(frame: Uint8Array<ArrayBuffer>): void;
  /** Leaves the room, also one a join still on its way will be let into, and hears no more. */
  close(): void;
}

/**
 * A way into a card's room: the page's connection (`api/live-rooms.ts`) or
 * the rooms of the backend in memory (`state/memory/rooms.ts`). Nothing is
 * told before the port is returned.
 */
export type RoomLine = (hears: RoomHears) => RoomPort;

export type RoomTimer = 'flush' | 'reset' | 'retry' | 'answer';

export type RoomEvent =
  /** The connection is there, or is gone. */
  | { type: 'up' }
  | { type: 'down' }
  | { type: 'message'; message: RoomMessage }
  | { type: 'frame'; frame: RoomFrame }
  /** The reader changed the document held. */
  | { type: 'local'; update: Uint8Array }
  /** The answer to `offer`: what the document held has that the server's lacks, if anything. */
  | { type: 'missing'; update: Uint8Array | undefined }
  | { type: 'timer'; timer: RoomTimer }
  /** The reader asks to be let in again. */
  | { type: 'retry' }
  /** Everything waiting is to be sent now: the card is being left. */
  | { type: 'settle' };

export type RoomEffect =
  /** Send `JOIN_DOC`. */
  | { type: 'join' }
  | { type: 'send'; frame: RoomFrame }
  /** Merge into the document held. */
  | { type: 'apply'; update: Uint8Array }
  /** Drop the document held, if any, and hold a new one made of this. */
  | { type: 'fresh'; update: Uint8Array }
  /** Drop the document held: there is none to show. */
  | { type: 'drop' }
  /** Say (`missing`) what the document held has that `have` lacks. */
  | { type: 'offer'; have: Uint8Array }
  /** Changes of the reader were dropped with their document; `why` is a reason of the server's, or `document_reset`. */
  | { type: 'discarded'; why: string }
  | { type: 'timer'; timer: RoomTimer; wait: number }
  | { type: 'cancel'; timer: RoomTimer }
  /** `status` is another now. */
  | { type: 'changed' };

export type RoomPhase = 'offline' | 'joining' | 'joined' | 'receiving' | 'out';

export interface RoomStatus {
  phase: RoomPhase;
  mode: DocMode;
  /** Why it is `out`. */
  reason: string | undefined;
  /** A document is held. */
  held: boolean;
  /** The reader may write now: joined, to write, and not waiting for the document to be sent again. */
  writable: boolean;
  /** The document held has changes of the reader the server is not known to have. */
  pending: boolean;
  /** Being `out`, it is tried again by itself. */
  retrying: boolean;
}
