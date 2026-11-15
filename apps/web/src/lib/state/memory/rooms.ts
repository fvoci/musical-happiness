/**
 * The rooms of the bodies edited together, as the server keeps them
 * (`contracts/websocket.md`, section 6), for the backend in memory: one Y
 * document a card, made from its body on the first joining, and the
 * readers in its room. They speak the server's messages and frames to the
 * same room client (`doc-client.ts`), so that what the screens do is what
 * they do against the server, without one.
 *
 * - A reader is let in to write with a role that edits the board, to read
 *   with any other role, and not at all without one (`not_found`).
 * - A change is checked as the server would (read only, another `epoch`,
 *   too large, not a body, resting on what the document lacks) and refused
 *   with its reason; one taken is answered and sent to the room's others.
 * - The card's body is written from the document 2 seconds after the last
 *   change, 10 at most after the first, and when the last who writes leaves:
 *   the board's listeners are told the card, as `UPDATE_BLOCK` tells them.
 * - A body written whole (`MemoryBackend.writeCardBody`, a version
 *   restored) is written after the room's changes, and makes the document
 *   anew (`DOC_RESET`).
 * - Versions of the body are kept as the room is written in and left
 *   (`body-versions.ts`).
 *
 * The tabs of one browser that each run a world in memory can be joined
 * (`shareRooms`): their documents are made alike from the seed, so that the
 * changes of each go to the others' as they would through one server. Two
 * rooms are one only when they are of the same card (board, identifier and
 * time of making: the worlds make identifiers alike) and their documents
 * were made from the same body in the same epoch.
 *
 * Unlike the server, a change that would leave the document no body is
 * refused (`update_refused`), not taken and mended; limits of frames,
 * rooms and connections are not kept; the roles are asked again when the
 * members change, not on a timer; and a room is kept for the life of the
 * world, as the server keeps a document stored.
 */
import * as Y from 'yjs';

import { BODY_FIELD } from '../../editor/body-editor.ts';
import { storedSize } from '../../editor/schema.ts';
import {
  decodeDocFrame,
  encodeDocFrame,
  SYNC_STEP_1,
  SYNC_STEP_2,
  SYNC_UPDATE,
} from '../../logic/doc-frame.ts';
import type {
  DocMode,
  RoomHears,
  RoomLine,
  RoomMessage,
  RoomPort,
} from '../../logic/doc-room-types.ts';
import { ROOM_LIMITS } from '../../logic/doc-room.ts';

import { leftByWriters, roomWrote, typedBy, writersLeft } from './body-versions.ts';
import { roleOn } from './comments.ts';
import { changeOf, imageFileIds, LIMITS } from './shapes.ts';
import { bodyOf, documentOf, sameBody } from './room-docs.ts';
import type { StoredCard, World } from './world.ts';

/** The shared type the editor binds to (`@tiptap/extension-collaboration`). */
export const FIELD = BODY_FIELD;
export { bodyOf } from './room-docs.ts';
/** When a body is written after a change: after a pause, at most after the first change. */
const QUIET_WAIT = 2000;
const LONGEST_WAIT = 10_000;

/** Who goes through a room, as the backend in memory acts for them. */
export interface RoomUser {
  readonly userId: string;
  /** Milliseconds every answer waits. */
  readonly latency: number;
}

interface Seat {
  user: RoomUser;
  hears: RoomHears;
  /** The room's number while the seat is in it. */
  doc: number | undefined;
  mode: DocMode;
  /** In the room's view: until what the seat said before leaving has been taken. */
  up: boolean;
  /** The seat's client still listens: nothing is said to it once it has closed. */
  hearing: boolean;
  /** When the last thing each way arrives: nothing overtakes what was sent before it. */
  inAt: number;
  outAt: number;
}

interface Room {
  boardId: string;
  cardId: string;
  doc: Y.Doc;
  epoch: number;
  seats: Set<Seat>;
  /** Who changed it last: the body is written in their name. */
  lastWriter: string | undefined;
  quiet: ReturnType<typeof setTimeout> | undefined;
  longest: ReturnType<typeof setTimeout> | undefined;
  /** What another tab's room has to be to be this one (`tabKey`). */
  tab: string;
}

/** What the worlds of two tabs say of a room; of a room another than this tab's (`tab`) it is not heard. */
type TabMessage =
  | { kind: 'update'; cardId: string; tab: string; update: Uint8Array }
  | { kind: 'ask'; cardId: string; tab: string; vector: Uint8Array };

/** A short sum of a text (FNV-1a), to tell apart bodies a document was made from. */
function sum(text: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash = Math.imul(hash ^ text.charCodeAt(index), 0x01000193) >>> 0;
  }
  return hash.toString(36);
}

/** What makes two tabs' rooms one: the card, its making, the epoch and the body the document came from. */
function tabKey(card: StoredCard | undefined, cardId: string, epoch: number): string {
  const made = sum(JSON.stringify(card?.body ?? null));
  return `${card?.boardId ?? ''} ${cardId} ${String(card?.createAt ?? 0)} ${String(epoch)} ${made}`;
}

export class MemoryRooms {
  readonly #world: World;
  readonly #rooms = new Map<string, Room>();
  #numbers = 0;
  #channel: BroadcastChannel | undefined;

  constructor(world: World, channel?: string) {
    this.#world = world;
    world.bodyFlushed = (cardId) => {
      const room = this.#rooms.get(cardId);
      if (room) this.#write(room);
    };
    world.bodyUnwritten = (cardId) => {
      const room = this.#rooms.get(cardId);
      const body = room && bodyOf(room.doc);
      const card = room && this.#card(room.boardId, cardId);
      return !!card && body !== undefined && !sameBody(body, card.body ?? null);
    };
    world.bodyWritten = (cardId) => {
      this.#remake(cardId);
    };
    if (channel !== undefined && typeof BroadcastChannel !== 'undefined') {
      this.#channel = new BroadcastChannel(channel);
      this.#channel.onmessage = (event: MessageEvent<TabMessage>) => {
        this.#fromTab(event.data);
      };
    }
  }

  /** A way into a card's room for a user. */
  line(user: RoomUser, boardId: string, cardId: string): RoomLine {
    return (hears) => {
      const seat: Seat = {
        user,
        hears,
        doc: undefined,
        mode: 'read',
        up: true,
        hearing: true,
        inAt: 0,
        outAt: 0,
      };
      const port: RoomPort = {
        connected: true,
        // What a seat says reaches the room after the user's latency, as over a network.
        join: () => {
          this.#hear(seat, () => {
            this.#join(seat, boardId, cardId);
          });
        },
        send: (bytes) => {
          this.#hear(seat, () => {
            this.#take(seat, cardId, bytes);
          });
        },
        // Leaving comes after what was sent before it, as `LEAVE_DOC` does.
        close: () => {
          seat.hearing = false;
          this.#hear(seat, () => {
            seat.up = false;
            this.#leave(seat, cardId);
          });
        },
      };
      return port;
    };
  }

  /** How many readers are in a card's room (for tests). */
  members(cardId: string): number {
    return this.#rooms.get(cardId)?.seats.size ?? 0;
  }

  /** Writes every room's body now (for tests: as if the waits had passed). */
  flush(): void {
    for (const room of this.#rooms.values()) this.#write(room);
  }

  /** Takes what a seat said after the user's latency, in the order said; not once it has left. */
  #hear(seat: Seat, take: () => void): void {
    seat.inAt = Math.max(Date.now() + seat.user.latency, seat.inAt);
    setTimeout(() => {
      if (seat.up) take();
    }, seat.inAt - Date.now());
  }

  /** Says something to a seat after the user's latency, in the order said. */
  #say(seat: Seat, what: { message: RoomMessage } | { frame: Uint8Array }): void {
    seat.outAt = Math.max(Date.now() + seat.user.latency, seat.outAt);
    setTimeout(() => {
      if (!seat.hearing) return;
      if ('message' in what) seat.hears.message(what.message);
      else {
        const frame = decodeDocFrame(what.frame);
        if (frame) seat.hears.frame(frame);
      }
    }, seat.outAt - Date.now());
  }

  #card(boardId: string, cardId: string): StoredCard | undefined {
    const block = this.#world.blocks.get(cardId);
    if ((block?.type !== 'card' && block?.type !== 'page') || block.boardId !== boardId) return;
    return block as StoredCard;
  }

  /** The way the user may be in the room now; undefined when not at all. */
  #modeOf(user: RoomUser, boardId: string, cardId: string): DocMode | undefined {
    if (!this.#card(boardId, cardId)) return undefined;
    const role = roleOn(this.#world.members.get(boardId) ?? [], user.userId);
    if (role === '') return undefined;
    return role === 'admin' || role === 'editor' ? 'write' : 'read';
  }

  #room(boardId: string, cardId: string): Room {
    const known = this.#rooms.get(cardId);
    if (known) return known;
    const card = this.#card(boardId, cardId);
    const doc = documentOf(card?.body ?? null);
    const room: Room = {
      boardId,
      cardId,
      doc,
      epoch: 1,
      seats: new Set(),
      lastWriter: undefined,
      quiet: undefined,
      longest: undefined,
      tab: tabKey(card, cardId, 1),
    };
    this.#world.watch({
      boardId,
      userId: '',
      listener: (change) => {
        if (change.kind === 'members') this.#recheck(room);
      },
    });
    this.#rooms.set(cardId, room);
    // Another tab may have written in it already.
    this.#post({ kind: 'ask', cardId, tab: room.tab, vector: Y.encodeStateVector(doc) });
    return room;
  }

  #post(message: TabMessage): void {
    this.#channel?.postMessage(message);
  }

  #join(seat: Seat, boardId: string, cardId: string): void {
    const mode = this.#modeOf(seat.user, boardId, cardId);
    // Refused, the room before (if any) stays the seat's.
    if (mode === undefined) {
      this.#say(seat, { message: { action: 'DOC_REFUSED', reason: 'not_found' } });
      return;
    }
    const room = this.#room(boardId, cardId);
    this.#numbers += 1;
    const doc = this.#numbers;
    seat.doc = doc;
    seat.mode = mode;
    room.seats.add(seat);
    this.#say(seat, { message: { action: 'DOC_JOINED', doc, epoch: room.epoch, mode } });
    this.#sendWhole(seat, room, Y.encodeStateAsUpdate(room.doc));
  }

  #sendWhole(seat: Seat, room: Room, update: Uint8Array): void {
    const doc = seat.doc;
    if (doc === undefined) return;
    this.#say(seat, {
      frame: encodeDocFrame({ doc, epoch: room.epoch, sync: SYNC_STEP_2, payload: update }),
    });
  }

  #leave(seat: Seat, cardId: string): void {
    const room = this.#rooms.get(cardId);
    seat.doc = undefined;
    if (!room?.seats.delete(seat)) return;
    this.#afterLeft(room);
  }

  /** Nobody who writes is in the room any more, or none who wrote since the newest version: its body is written, and what was written is kept as a version. */
  #afterLeft(room: Room): void {
    const staying = [...room.seats].filter((seat) => seat.mode === 'write');
    const users = staying.map((seat) => seat.user.userId);
    if (staying.length > 0 && !leftByWriters(this.#world, room.cardId, users)) return;
    this.#write(room);
    const card = this.#card(room.boardId, room.cardId);
    if (card) writersLeft(this.#world, card);
  }

  #take(seat: Seat, cardId: string, bytes: Uint8Array): void {
    const room = this.#rooms.get(cardId);
    const frame = decodeDocFrame(bytes);
    const doc = seat.doc;
    if (!room || !frame || doc === undefined || frame.doc !== doc || !room.seats.has(seat)) return;
    if (frame.sync === SYNC_STEP_1) {
      if (frame.epoch !== room.epoch) return;
      try {
        this.#sendWhole(seat, room, Y.encodeStateAsUpdate(room.doc, frame.payload));
      } catch {
        // Not a state vector: no answer.
      }
      return;
    }
    const refusal = this.#refusal(seat, room, frame.epoch, frame.payload);
    if (refusal) {
      this.#say(seat, {
        message: { action: 'DOC_NACK', doc, epoch: frame.epoch, reason: refusal },
      });
      return;
    }
    const card = this.#card(room.boardId, room.cardId);
    if (card) typedBy(this.#world, card, seat.user.userId);
    Y.applyUpdate(room.doc, frame.payload, seat);
    room.lastWriter = seat.user.userId;
    this.#say(seat, { message: { action: 'DOC_ACK', doc, epoch: frame.epoch } });
    this.#spread(room, frame.payload, seat);
    this.#post({ kind: 'update', cardId, tab: room.tab, update: frame.payload });
    this.#changed(room);
  }

  /** Why the server would not take a change from a seat; undefined when it would. */
  #refusal(seat: Seat, room: Room, epoch: number, update: Uint8Array): string | undefined {
    if (seat.mode === 'read') return 'document_read_only';
    if (epoch !== room.epoch) return 'update_out_of_step';
    if (update.length > ROOM_LIMITS.updateBytes) return 'update_too_large';
    // A card deleted meanwhile: the room stays until the roles are asked again.
    if (!this.#card(room.boardId, room.cardId)) return 'not_found';
    return this.#check(room, update);
  }

  /** Why the server would not take an update into the document; undefined when it would. */
  #check(room: Room, update: Uint8Array): string | undefined {
    const probe = new Y.Doc();
    try {
      Y.applyUpdate(probe, Y.encodeStateAsUpdate(room.doc));
      Y.applyUpdate(probe, update);
    } catch {
      return 'update_refused';
    }
    if (probe.store.pendingStructs !== null || probe.store.pendingDs !== null) {
      return 'update_out_of_step';
    }
    const body = bodyOf(probe);
    if (body === undefined) return 'update_refused';
    if (body === null) return undefined;
    if (storedSize(body) > LIMITS.bodyBytes) return 'document_too_large';
    // An image shows a file of this board, or none.
    const files = this.#world.files;
    if (imageFileIds(body).some((fileId) => files.get(fileId)?.boardId !== room.boardId)) {
      return 'update_refused';
    }
    return undefined;
  }

  /** An update the room took goes to every seat in it but the one it came from. */
  #spread(room: Room, update: Uint8Array, from: Seat | undefined): void {
    for (const other of room.seats) {
      if (other === from || other.doc === undefined) continue;
      this.#say(other, {
        frame: encodeDocFrame({
          doc: other.doc,
          epoch: room.epoch,
          sync: SYNC_UPDATE,
          payload: update,
        }),
      });
    }
  }

  #changed(room: Room): void {
    clearTimeout(room.quiet);
    room.quiet = setTimeout(() => {
      this.#write(room);
    }, QUIET_WAIT);
    room.longest ??= setTimeout(() => {
      this.#write(room);
    }, LONGEST_WAIT);
  }

  /** Writes the body from the document, if it says something else, and tells the board. */
  #write(room: Room): void {
    clearTimeout(room.quiet);
    clearTimeout(room.longest);
    room.quiet = undefined;
    room.longest = undefined;
    const card = this.#card(room.boardId, room.cardId);
    const body = bodyOf(room.doc);
    if (!card || body === undefined) return;
    if (sameBody(body, card.body ?? null)) return;
    card.body = body;
    card.updateAt = this.#world.now();
    card.modifiedBy = room.lastWriter ?? card.modifiedBy;
    this.#world.tellBoard(room.boardId, changeOf(room.boardId, card));
    roomWrote(this.#world, card);
  }

  /** The card's body was written whole: the room's document is made anew from it, of the next epoch. */
  #remake(cardId: string): void {
    const room = this.#rooms.get(cardId);
    const card = this.#card(room?.boardId ?? '', cardId);
    // As the server: a body written whole that is what the document holds changes nothing.
    if (!room || sameBody(bodyOf(room.doc) ?? null, card?.body ?? null)) return;
    clearTimeout(room.quiet);
    clearTimeout(room.longest);
    room.quiet = undefined;
    room.longest = undefined;
    room.doc.destroy();
    room.doc = documentOf(card?.body ?? null);
    room.epoch += 1;
    room.tab = tabKey(card, cardId, room.epoch);
    const whole = Y.encodeStateAsUpdate(room.doc);
    for (const seat of room.seats) {
      const doc = seat.doc;
      if (doc === undefined) continue;
      this.#say(seat, { message: { action: 'DOC_RESET', doc, epoch: room.epoch } });
      this.#sendWhole(seat, room, whole);
    }
  }

  /** The roles were changed: each seat is let stay as it may now, or put out. */
  #recheck(room: Room): void {
    for (const seat of [...room.seats]) {
      const doc = seat.doc;
      if (doc === undefined) continue;
      const mode = this.#modeOf(seat.user, room.boardId, room.cardId);
      if (mode === undefined) {
        room.seats.delete(seat);
        seat.doc = undefined;
        this.#say(seat, { message: { action: 'DOC_LEFT', doc, reason: 'not_found' } });
      } else if (mode !== seat.mode) {
        seat.mode = mode;
        this.#say(seat, { message: { action: 'DOC_MODE', doc, mode } });
      }
    }
    this.#afterLeft(room);
  }

  /** What another tab's world said. */
  #fromTab(message: TabMessage): void {
    const room = this.#rooms.get(message.cardId);
    if (room?.tab !== message.tab) return;
    if (message.kind === 'ask') {
      const update = Y.encodeStateAsUpdate(room.doc, message.vector);
      this.#post({ kind: 'update', cardId: room.cardId, tab: room.tab, update });
      return;
    }
    Y.applyUpdate(room.doc, message.update, this);
    this.#spread(room, message.update, undefined);
    this.#changed(room);
  }
}

/** The rooms of a world: one set for every backend that acts on it. */
const ofWorld = new WeakMap<World, MemoryRooms>();

export function roomsOf(world: World): MemoryRooms {
  let rooms = ofWorld.get(world);
  if (!rooms) {
    rooms = new MemoryRooms(world, world.shareRooms);
    ofWorld.set(world, rooms);
  }
  return rooms;
}
