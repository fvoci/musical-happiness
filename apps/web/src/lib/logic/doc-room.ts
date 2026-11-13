/**
 * One room of a card's body edited together, as its client sees it: a state
 * machine with no input or output of its own (`contracts/websocket.md`,
 * section 6). It is told what happens (`take`) and answers what is to be
 * done (`RoomEffect`); whoever runs it owns the connection, the timers and
 * the Y document, of which this knows only bytes.
 *
 * The phases:
 *
 * - `offline`: no connection. A document held is kept, to go on from.
 * - `joining`: asked to be let in, no document held yet.
 * - `joined`: in the room, with the document; `mode` says whether it may write.
 * - `receiving`: a document is held and another whole one is on its way
 *   (let in again, or the server made the document anew). Nothing is sent.
 * - `out`: not in the room, with the server's reason. Some reasons are tried
 *   again after a wait; `not_found` is not.
 *
 * The document is received again in four cases. `DOC_RESET`: a new `epoch`
 * is another document (the one held is dropped), the same is merged.
 * `DOC_NACK`: the refused change is in the reader's document alone, so the
 * room is joined once more and the document dropped for the server's (a
 * change the server only could not take just now is kept and offered
 * again). `DOC_LEFT`, and a connection that came back: joined again, and a
 * document of the same `epoch` goes on with what the server lacks of it.
 * Whenever a document is dropped with changes of the reader the server never
 * took, that is said (`discarded`).
 */

import { packUpdates } from './doc-frame.ts';
import type { DocMode, RoomFrame, RoomMessage } from './doc-room-types.ts';

export type * from './doc-room-types.ts';

const STEP_2 = 1;
const UPDATE = 2;

export type RoomTimer = 'flush' | 'reset' | 'retry';

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

export interface RoomLimits {
  /** The largest update the server takes, in bytes (`collab_update_bytes`). */
  updateBytes: number;
}

export const ROOM_LIMITS: RoomLimits = { updateBytes: 64 * 1024 };

/**
 * How long changes are gathered into one frame, and the pause between two
 * frames, in milliseconds: at most 33 frames a second where the server
 * allows 50.
 */
export const FLUSH_WAIT = 30;
/** How long a refusal as out of step waits for the `DOC_RESET` that explains it. */
export const RESET_WAIT = 1500;
/** The wait before joining again after being put out for sending or reading at the wrong pace. */
export const PACE_WAIT = 1000;
/** The waits before trying again what was refused for now, by tries; the last repeats. */
export const RETRY_WAITS = [1000, 2000, 5000, 15_000, 30_000];

/** Reasons that do not pass by waiting: the card is not there for the reader, or the page is in too many rooms. */
const FINAL = new Set(['not_found', 'too_many_documents_joined']);
/** Being put out for these says nothing against the document: the room is joined again as it was left. */
const PACE = new Set(['too_many_frames', 'connection_too_slow']);

export class DocRoom {
  readonly #merge: (updates: Uint8Array[]) => Uint8Array;
  readonly #limits: RoomLimits;
  #up = false;
  #phase: RoomPhase = 'offline';
  #mode: DocMode = 'read';
  #reason: string | undefined;
  /** The room's number on this connection, while it has one. */
  #doc: number | undefined;
  /** The epoch of the document held. */
  #epoch: number | undefined;
  /** The next frame of the room is the whole document of this epoch. */
  #expect: number | undefined;
  /** A whole document of the epoch held is merged into it; otherwise it takes its place. */
  #keep = true;
  /** Why the document held is not kept. */
  #why = '';
  /** A refusal as out of step waits for its `DOC_RESET`. */
  #waiting = false;
  /** Changes not yet put into a frame, frames not yet sent, frames not yet answered. */
  #buffer: Uint8Array[] = [];
  #outbox: Uint8Array[] = [];
  #unacked = 0;
  #pending = false;
  /** A document read only was changed here (the editor's own repair): it is not the server's any more. */
  #tainted = false;
  #tries = 0;
  #retrying = false;
  #effects: RoomEffect[] = [];

  constructor(merge: (updates: Uint8Array[]) => Uint8Array, limits: RoomLimits = ROOM_LIMITS) {
    this.#merge = merge;
    this.#limits = limits;
  }

  get status(): RoomStatus {
    return {
      phase: this.#phase,
      mode: this.#mode,
      reason: this.#reason,
      held: this.#epoch !== undefined,
      writable: this.#phase === 'joined' && this.#mode === 'write' && !this.#waiting,
      pending: this.#pending,
      retrying: this.#retrying,
    };
  }

  /** What happened; answers what is to be done, in order. */
  take(event: RoomEvent): RoomEffect[] {
    const before = this.status;
    this.#effects = [];
    switch (event.type) {
      case 'up':
        this.#up = true;
        this.#join();
        break;
      case 'down':
        this.#up = false;
        this.#leaveRoom();
        this.#cancel('retry');
        this.#retrying = false;
        this.#phase = 'offline';
        break;
      case 'message':
        this.#message(event.message);
        break;
      case 'frame':
        this.#frame(event.frame);
        break;
      case 'local':
        this.#local(event.update);
        break;
      case 'missing':
        this.#missing(event.update);
        break;
      case 'timer':
        this.#timer(event.timer);
        break;
      case 'retry':
        this.#tries = 0;
        this.#timer('retry');
        break;
      case 'settle':
        this.#flush(true);
        break;
    }
    const after = this.status;
    const keys = Object.keys(after) as (keyof RoomStatus)[];
    if (keys.some((key) => after[key] !== before[key])) this.#effects.push({ type: 'changed' });
    return this.#effects;
  }

  #cancel(timer: RoomTimer): void {
    this.#effects.push({ type: 'cancel', timer });
  }

  /** Asks to be let in. The number held, if any, stays the room's until the answer. */
  #join(): void {
    if (!this.#up) return;
    this.#cancel('retry');
    this.#retrying = false;
    this.#expect = undefined;
    this.#phase = this.#epoch === undefined ? 'joining' : 'receiving';
    this.#effects.push({ type: 'join' });
  }

  /** Forgets the room's number and what waited to go to it: none of it will be answered. */
  #leaveRoom(): void {
    this.#doc = undefined;
    this.#expect = undefined;
    this.#waiting = false;
    this.#cancel('reset');
    this.#unqueue();
  }

  #unqueue(): void {
    this.#buffer = [];
    this.#outbox = [];
    this.#unacked = 0;
    this.#cancel('flush');
  }

  /** Joins again for the server's document: with `keep`, to go on from the one held. */
  #rejoin(keep: boolean, why = ''): void {
    this.#leaveRoom();
    if (!keep) {
      this.#keep = false;
      this.#why = why;
    }
    if (this.#up) this.#join();
  }

  /** Not in the room, for `reason`: for good, or until the next try. */
  #out(reason: string): void {
    this.#leaveRoom();
    this.#phase = 'out';
    this.#reason = reason;
    if (FINAL.has(reason)) {
      this.#cancel('retry');
      this.#retrying = false;
      this.#drop(reason);
      return;
    }
    const wait = RETRY_WAITS[Math.min(this.#tries, RETRY_WAITS.length - 1)] ?? 30_000;
    this.#tries += 1;
    this.#retrying = true;
    this.#effects.push({ type: 'timer', timer: 'retry', wait });
  }

  /** Lets go of the document held, saying so when changes of the reader go with it. */
  #drop(why: string): void {
    if (this.#epoch === undefined) return;
    if (this.#pending) this.#effects.push({ type: 'discarded', why });
    this.#pending = false;
    this.#tainted = false;
    this.#epoch = undefined;
    this.#keep = true;
    this.#effects.push({ type: 'drop' });
  }

  #message(message: RoomMessage): void {
    if (message.action === 'DOC_JOINED') {
      this.#leaveRoom();
      this.#doc = message.doc;
      this.#mode = message.mode;
      this.#expect = message.epoch;
      this.#tries = 0;
      return;
    }
    if (message.action === 'DOC_REFUSED') {
      // Still held: an answer to the try of a room that is out, or to joining again.
      if (PACE.has(message.reason)) this.#paced();
      else this.#out(message.reason);
      return;
    }
    // Of a room this is no longer in, or never was.
    if (message.doc !== this.#doc) return;
    switch (message.action) {
      case 'DOC_ACK':
        if (this.#answersDropped(message.epoch)) return;
        this.#unacked = Math.max(0, this.#unacked - 1);
        this.#settled();
        break;
      case 'DOC_NACK':
        if (this.#answersDropped(message.epoch)) return;
        this.#refused(message.reason);
        break;
      case 'DOC_MODE':
        if (message.mode === 'write' && this.#mode === 'read' && this.#tainted) {
          this.#mode = 'write';
          this.#rejoin(false);
        } else this.#mode = message.mode;
        break;
      case 'DOC_RESET':
        this.#cancel('reset');
        this.#expect = message.epoch;
        if (message.epoch !== this.#epoch) {
          this.#unqueue();
          this.#keep = false;
          this.#why = 'document_reset';
          this.#waiting = false;
          this.#phase = 'receiving';
        }
        break;
      case 'DOC_LEFT':
        if (PACE.has(message.reason)) this.#paced();
        else this.#out(message.reason);
        break;
    }
  }

  /**
   * Whether an answer of the server is about changes already given up: of
   * another document than the one held, of the one held while another is on
   * its way (`DOC_RESET` to a new `epoch`), or while a refusal waits for its
   * `DOC_RESET`.
   */
  #answersDropped(epoch: number): boolean {
    if (epoch !== this.#epoch || this.#waiting) return true;
    return this.#expect !== undefined && this.#expect !== this.#epoch;
  }

  /** Put out, or kept out, for the pace of the frames: joined again after a pause, as it was left. */
  #paced(): void {
    this.#leaveRoom();
    this.#phase = this.#epoch === undefined ? 'joining' : 'receiving';
    this.#effects.push({ type: 'timer', timer: 'retry', wait: PACE_WAIT });
  }

  /** The server did not take a change: it is in the document held and nowhere else. */
  #refused(reason: string): void {
    if (reason === 'update_out_of_step') {
      // The document may just have been made anew: its `DOC_RESET` is on its way.
      this.#unqueue();
      this.#waiting = true;
      this.#effects.push({ type: 'timer', timer: 'reset', wait: RESET_WAIT });
    } else if (reason === 'document_unavailable') {
      // Nothing is wrong with the change: it is offered again once let in.
      this.#leaveRoom();
      this.#phase = 'receiving';
      this.#effects.push({ type: 'timer', timer: 'retry', wait: PACE_WAIT });
    } else {
      if (reason === 'document_read_only') this.#mode = 'read';
      this.#rejoin(false, reason);
    }
  }

  #frame(frame: RoomFrame): void {
    if (frame.doc !== this.#doc) return;
    if (this.#expect !== undefined) {
      // Of the document before: the whole of the new one is the first frame of its epoch.
      if (frame.epoch !== this.#expect || frame.sync !== STEP_2) return;
      this.#whole(frame.payload, frame.epoch);
      return;
    }
    if (frame.epoch !== this.#epoch) return;
    if (frame.sync === STEP_2 || frame.sync === UPDATE) {
      this.#effects.push({ type: 'apply', update: frame.payload });
    }
  }

  #whole(update: Uint8Array, epoch: number): void {
    this.#expect = undefined;
    const resumed = this.#waiting;
    this.#waiting = false;
    if (this.#epoch === epoch && this.#keep) {
      this.#effects.push({ type: 'apply', update });
      // The same document: what the server lacks of the reader's changes goes to it again.
      if (this.#pending && (this.#phase !== 'joined' || resumed)) {
        this.#unqueue();
        this.#effects.push({ type: 'offer', have: update });
      }
    } else {
      if (this.#pending)
        this.#effects.push({ type: 'discarded', why: this.#why || 'document_reset' });
      this.#pending = false;
      this.#tainted = false;
      this.#unqueue();
      this.#epoch = epoch;
      this.#effects.push({ type: 'fresh', update });
    }
    this.#keep = true;
    this.#why = '';
    this.#reason = undefined;
    this.#phase = 'joined';
  }

  #missing(update: Uint8Array | undefined): void {
    if (this.#phase !== 'joined' || this.#waiting) return;
    if (update === undefined) {
      this.#settled();
      return;
    }
    // Let in to read only since: the changes cannot go, and the document is not the server's.
    if (this.#mode === 'read') this.#rejoin(false, 'document_read_only');
    else this.#queue(update);
  }

  #local(update: Uint8Array): void {
    if (this.#epoch === undefined) return;
    if (this.#phase === 'joined' && !this.#waiting) {
      if (this.#mode === 'write') this.#queue(update);
      else this.#tainted = true;
      return;
    }
    // Written while nothing can be sent: it goes with the rest when the room is joined again.
    this.#pending = true;
  }

  #queue(update: Uint8Array): void {
    const first = this.#buffer.length === 0 && this.#outbox.length === 0;
    this.#buffer.push(update);
    this.#pending = true;
    if (first) this.#effects.push({ type: 'timer', timer: 'flush', wait: FLUSH_WAIT });
  }

  #settled(): void {
    if (this.#phase !== 'joined' || this.#waiting) return;
    if (this.#unacked === 0 && this.#buffer.length === 0 && this.#outbox.length === 0) {
      this.#pending = false;
    }
  }

  #timer(timer: RoomTimer): void {
    if (timer === 'flush') this.#flush(false);
    else if (timer === 'reset') {
      if (this.#waiting) this.#rejoin(false, 'update_out_of_step');
    } else if (this.#doc === undefined && this.#phase !== 'joined') this.#join();
  }

  /** Puts what was gathered into frames and sends the next one; with `all`, every one. */
  #flush(all: boolean): void {
    const doc = this.#doc;
    const epoch = this.#epoch;
    if (this.#phase !== 'joined' || this.#mode !== 'write' || this.#waiting) return;
    if (doc === undefined || epoch === undefined) return;
    if (this.#buffer.length > 0) {
      const frames = packUpdates(this.#buffer, this.#limits.updateBytes, this.#merge);
      if (!frames) {
        // One change larger than the server takes: it cannot stay in the document.
        this.#rejoin(false, 'update_too_large');
        return;
      }
      this.#buffer = [];
      this.#outbox.push(...frames);
    }
    for (let payload = this.#outbox.shift(); payload; payload = this.#outbox.shift()) {
      this.#effects.push({ type: 'send', frame: { doc, epoch, sync: UPDATE, payload } });
      this.#unacked += 1;
      if (!all) break;
    }
    if (this.#outbox.length > 0) {
      this.#effects.push({ type: 'timer', timer: 'flush', wait: FLUSH_WAIT });
    }
  }
}
