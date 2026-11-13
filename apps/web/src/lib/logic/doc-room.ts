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

import { packUpdates, SYNC_STEP_2 as STEP_2, SYNC_UPDATE as UPDATE } from './doc-frame.ts';
import type {
  DocMode,
  RoomEffect,
  RoomEvent,
  RoomFrame,
  RoomMessage,
  RoomPhase,
  RoomStatus,
  RoomTimer,
} from './doc-room-types.ts';

export type * from './doc-room-types.ts';

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
const RETRY_WAITS = [1000, 2000, 5000, 15_000, 30_000];

/** How long a join waits for its answer before it is taken as refused for now. */
const ANSWER_WAIT = 10_000;

/** A reason that does not pass by waiting: the card is not there for the reader. */
const FINAL = new Set(['not_found']);
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
  /** A `JOIN_DOC` is on its way: another is not sent until it is answered. */
  #joinSent = false;
  /** Answers still to come for frames given up while the room's number stayed: they are about nothing now. */
  #stale = 0;
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
        // Out for good stays out; anything else is joined anew on the new connection.
        if (!this.#outForGood()) this.#join();
        break;
      case 'down':
        this.#up = false;
        this.#answered();
        this.#leaveRoom();
        this.#cancel('retry');
        this.#retrying = false;
        if (!this.#outForGood()) this.#phase = 'offline';
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

  #outForGood(): boolean {
    return this.#phase === 'out' && FINAL.has(this.#reason ?? '');
  }

  #cancel(timer: RoomTimer): void {
    this.#effects.push({ type: 'cancel', timer });
  }

  /** Asks to be let in. The number held, if any, stays the room's until the answer. */
  #join(): void {
    if (!this.#up || this.#joinSent) return;
    this.#cancel('retry');
    this.#retrying = false;
    this.#expect = undefined;
    this.#phase = this.#epoch === undefined ? 'joining' : 'receiving';
    this.#joinSent = true;
    this.#effects.push({ type: 'join' });
    this.#effects.push({ type: 'timer', timer: 'answer', wait: ANSWER_WAIT });
  }

  /** The join on its way was answered, or will not be. */
  #answered(): void {
    this.#joinSent = false;
    this.#cancel('answer');
  }

  /** Forgets the room's number and what waited to go to it: none of it will be answered. */
  #leaveRoom(): void {
    this.#doc = undefined;
    this.#stale = 0;
    this.#expect = undefined;
    this.#waiting = false;
    this.#cancel('reset');
    this.#unqueue();
  }

  #unqueue(): void {
    this.#buffer = [];
    this.#outbox = [];
    // In the same room, the answers to what was sent still come: they are not about what is sent next.
    if (this.#doc !== undefined) this.#stale += this.#unacked;
    this.#unacked = 0;
    this.#cancel('flush');
  }

  /** Joins again for the server's document, dropping the one held: what it had of the reader's is lost for `why`. */
  #rejoin(why: string): void {
    this.#leaveRoom();
    this.#keep = false;
    this.#why = why;
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
      this.#answered();
      this.#leaveRoom();
      this.#doc = message.doc;
      this.#mode = message.mode;
      this.#expect = message.epoch;
      this.#tries = 0;
      // Nothing is written until the whole document has come.
      if (this.#epoch !== undefined) this.#phase = 'receiving';
      return;
    }
    if (message.action === 'DOC_REFUSED') {
      this.#answered();
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
        // Answered: whatever is given up next does not wait for this answer.
        this.#unacked = Math.max(0, this.#unacked - 1);
        this.#refused(message.reason);
        break;
      case 'DOC_MODE':
        if (message.mode === 'write' && this.#mode === 'read' && this.#tainted) {
          this.#mode = 'write';
          this.#rejoin('');
        } else if (message.mode === 'read' && this.#buffer.length + this.#outbox.length > 0) {
          // What was gathered and not sent cannot go any more: it is the reader's document alone.
          this.#mode = 'read';
          this.#rejoin('document_read_only');
        } else this.#mode = message.mode;
        break;
      case 'DOC_RESET':
        this.#cancel('reset');
        this.#expect = message.epoch;
        if (message.epoch !== this.#epoch) {
          this.#unqueue();
          // What is still to be answered is of the old epoch, and is told apart by it.
          this.#stale = 0;
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
    // Of another document: told apart by its epoch alone.
    if (epoch !== this.#epoch) return true;
    if (this.#stale > 0) {
      this.#stale -= 1;
      return true;
    }
    if (this.#waiting) return true;
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
      this.#rejoin(reason);
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
    if (this.#mode === 'read') this.#rejoin('document_read_only');
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
      if (this.#waiting) this.#rejoin('update_out_of_step');
    } else if (timer === 'answer') {
      if (this.#joinSent) {
        this.#answered();
        this.#out('document_unavailable');
      }
    } else if (this.#doc === undefined && this.#phase !== 'joined') this.#join();
  }

  /** Puts what was gathered into frames and sends the next one; with `all`, every one. */
  #flush(all: boolean): void {
    const doc = this.#doc;
    const epoch = this.#epoch;
    if (this.#phase !== 'joined' || this.#mode !== 'write' || this.#waiting) return;
    if (doc === undefined || epoch === undefined) return;
    let tooLarge = false;
    if (this.#buffer.length > 0) {
      const limit = this.#limits.updateBytes;
      const over = this.#buffer.findIndex((update) => update.length > limit);
      // One change larger than the server takes cannot stay in the document;
      // what was written before it goes first.
      tooLarge = over !== -1;
      const fits = tooLarge ? this.#buffer.slice(0, over) : this.#buffer;
      this.#outbox.push(...(packUpdates(fits, limit, this.#merge) ?? []));
      this.#buffer = [];
    }
    for (let payload = this.#outbox.shift(); payload; payload = this.#outbox.shift()) {
      this.#effects.push({ type: 'send', frame: { doc, epoch, sync: UPDATE, payload } });
      this.#unacked += 1;
      if (!all && !tooLarge) break;
    }
    if (tooLarge) {
      this.#rejoin('update_too_large');
      return;
    }
    if (this.#outbox.length > 0) {
      this.#effects.push({ type: 'timer', timer: 'flush', wait: FLUSH_WAIT });
    }
  }
}
