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
 *   change, 10 at most after the first, and when the last reader leaves:
 *   the board's listeners are told the card, as `UPDATE_BLOCK` tells them.
 * - The roles are asked again whenever the board's members change.
 *
 * Unlike the server, a document is never made anew (no `DOC_RESET`): the
 * backend in memory has no way of writing a body whole any more. And the
 * tabs of one browser that each run a world in memory can be joined
 * (`shareRooms`): their documents are made alike from the seed, so that
 * the changes of each go to the others' as they would through one server.
 */
import { getSchema } from '@tiptap/core';
import { prosemirrorJSONToYXmlFragment, yXmlFragmentToProsemirrorJSON } from '@tiptap/y-tiptap';
import * as Y from 'yjs';

import { bodyExtensions } from '../../editor/extensions.ts';
import {
  emptyDoc,
  isEmptyDoc,
  toStoredDoc,
  validateDoc,
  type DocNode,
} from '../../editor/schema.ts';
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

import { roleOn } from './comments.ts';
import { LIMITS, wireOf } from './shapes.ts';
import type { StoredCard, World } from './world.ts';

/** The shared type the editor binds to (`@tiptap/extension-collaboration`). */
export const FIELD = 'default';
/** The largest update taken, in bytes (`collab_update_bytes`). */
export const UPDATE_BYTES = 64 * 1024;
/** When a body is written after a change: after a pause, at most after the first change. */
export const QUIET_WAIT = 2000;
export const LONGEST_WAIT = 10_000;
/** The client of a document made from a body: the same in every tab, so that two makings are one. */
const MAKER = 1;

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
  up: boolean;
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
  stopWatching: () => void;
}

type TabMessage =
  | { kind: 'update'; cardId: string; update: Uint8Array }
  | { kind: 'ask'; cardId: string; vector: Uint8Array };

const schema = getSchema(bodyExtensions());

/** The document of a body, made the same way wherever it is made. */
function documentOf(body: DocNode | null): Y.Doc {
  const doc = new Y.Doc();
  doc.clientID = MAKER;
  prosemirrorJSONToYXmlFragment(schema, body ?? emptyDoc(), doc.getXmlFragment(FIELD));
  // Whatever is written next is somebody's: never the maker's again.
  doc.clientID = Math.floor(Math.random() * 0x7fff_ffff) + 2;
  return doc;
}

/** The body a document holds, as it would be stored; undefined when it is not one. */
export function bodyOf(doc: Y.Doc): DocNode | null | undefined {
  try {
    const stored = toStoredDoc(yXmlFragmentToProsemirrorJSON(doc.getXmlFragment(FIELD)) as DocNode);
    if (validateDoc(stored).length > 0) return undefined;
    return isEmptyDoc(stored) ? null : stored;
  } catch {
    return undefined;
  }
}

export class MemoryRooms {
  readonly #world: World;
  readonly #rooms = new Map<string, Room>();
  #numbers = 0;
  #channel: BroadcastChannel | undefined;

  constructor(world: World, channel?: string) {
    this.#world = world;
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
      const seat: Seat = { user, hears, doc: undefined, mode: 'read', up: true };
      const port: RoomPort = {
        connected: true,
        join: () => {
          if (seat.up) this.#join(seat, boardId, cardId);
        },
        send: (bytes) => {
          if (seat.up) this.#take(seat, cardId, bytes);
        },
        close: () => {
          seat.up = false;
          this.#leave(seat, cardId);
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

  /** Says something to a seat after the user's latency, in the order said. */
  #say(seat: Seat, what: { message: RoomMessage } | { frame: Uint8Array }): void {
    setTimeout(() => {
      if (!seat.up) return;
      if ('message' in what) seat.hears.message(what.message);
      else {
        const frame = decodeDocFrame(what.frame);
        if (frame) seat.hears.frame(frame);
      }
    }, seat.user.latency);
  }

  #card(boardId: string, cardId: string): StoredCard | undefined {
    const block = this.#world.blocks.get(cardId);
    if (block?.type !== 'card' || block.boardId !== boardId) return undefined;
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
    const doc = documentOf(this.#card(boardId, cardId)?.body ?? null);
    const room: Room = {
      boardId,
      cardId,
      doc,
      epoch: 1,
      seats: new Set(),
      lastWriter: undefined,
      quiet: undefined,
      longest: undefined,
      stopWatching: this.#world.watch({
        boardId,
        userId: '',
        listener: (change) => {
          if (change.kind === 'members') this.#recheck(room);
        },
      }),
    };
    this.#rooms.set(cardId, room);
    // Another tab may have written in it already.
    this.#channel?.postMessage({ kind: 'ask', cardId, vector: Y.encodeStateVector(doc) });
    return room;
  }

  #join(seat: Seat, boardId: string, cardId: string): void {
    const mode = this.#modeOf(seat.user, boardId, cardId);
    // A join takes the place of the one before.
    if (seat.doc !== undefined) this.#rooms.get(cardId)?.seats.delete(seat);
    seat.doc = undefined;
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
    if (room.seats.size === 0) this.#write(room);
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
    Y.applyUpdate(room.doc, frame.payload, seat);
    room.lastWriter = seat.user.userId;
    this.#say(seat, { message: { action: 'DOC_ACK', doc, epoch: frame.epoch } });
    this.#spread(room, frame.payload, seat);
    this.#channel?.postMessage({ kind: 'update', cardId, update: frame.payload });
    this.#changed(room);
  }

  /** Why the server would not take a change from a seat; undefined when it would. */
  #refusal(seat: Seat, room: Room, epoch: number, update: Uint8Array): string | undefined {
    if (seat.mode === 'read') return 'document_read_only';
    if (epoch !== room.epoch) return 'update_out_of_step';
    if (update.length > UPDATE_BYTES) return 'update_too_large';
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
    if (body !== null && new TextEncoder().encode(JSON.stringify(body)).length > LIMITS.bodyBytes) {
      return 'document_too_large';
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
    if (JSON.stringify(body) === JSON.stringify(card.body ?? null)) return;
    card.body = body;
    card.updateAt = this.#world.now();
    card.modifiedBy = room.lastWriter ?? card.modifiedBy;
    this.#world.tellBoard(room.boardId, {
      kind: 'blocks',
      boardId: room.boardId,
      blocks: [wireOf(card)],
    });
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
    if (room.seats.size === 0) this.#write(room);
  }

  /** What another tab's world said. */
  #fromTab(message: TabMessage): void {
    const room = this.#rooms.get(message.cardId);
    if (!room) return;
    if (message.kind === 'ask') {
      const update = Y.encodeStateAsUpdate(room.doc, message.vector);
      this.#channel?.postMessage({ kind: 'update', cardId: room.cardId, update });
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
