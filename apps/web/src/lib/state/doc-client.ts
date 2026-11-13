/**
 * A card's body as a document edited together, on a way into its room
 * (`RoomLine`): the page's connection to the server, or the rooms of the
 * backend in memory. It runs the room's state machine (`logic/doc-room.ts`):
 * holds the Y document the machine speaks of only as bytes, keeps its
 * timers, and sends what it says.
 *
 * The document is the server's: nothing is ever made here from a body. This
 * file brings Yjs with it, so it is fetched when a body is first opened
 * (`doc-opener.ts`) and is no part of the first screen.
 */
import * as Y from 'yjs';

import { encodeDocFrame } from '../logic/doc-frame.ts';
import {
  DocRoom,
  type RoomEffect,
  type RoomEvent,
  type RoomLine,
  type RoomPort,
  type RoomTimer,
} from '../logic/doc-room.ts';

import type { BodyDoc, BodyDocHears, BodyDocState } from './backend.ts';

/** How long leaving a card waits for the server to take what was written, in milliseconds. */
const SETTLE_WAIT = 3000;

export class RoomClient implements BodyDoc {
  readonly #room = new DocRoom((updates) => Y.mergeUpdates(updates));
  readonly #hears: BodyDocHears;
  readonly #port: RoomPort;
  readonly #timers = new Map<RoomTimer, ReturnType<typeof setTimeout>>();
  /** Whoever waits for the server to have everything (`settle`). */
  #settling: (() => void)[] = [];
  #doc: Y.Doc | undefined;
  #closed = false;

  constructor(line: RoomLine, hears: BodyDocHears) {
    this.#hears = hears;
    this.#port = line({
      up: () => {
        this.#take({ type: 'up' });
      },
      down: () => {
        this.#take({ type: 'down' });
      },
      message: (message) => {
        this.#take({ type: 'message', message });
      },
      frame: (frame) => {
        this.#take({ type: 'frame', frame });
      },
    });
    if (this.#port.connected) this.#take({ type: 'up' });
  }

  get state(): BodyDocState {
    const { phase, mode, reason, writable, pending, retrying } = this.#room.status;
    return { status: phase, doc: this.#doc, mode, reason, writable, pending, retrying };
  }

  retry(): void {
    this.#take({ type: 'retry' });
  }

  /**
   * Sends what waits now, and answers once the server has everything the
   * reader wrote, or cannot be given it for now (no connection, not in the
   * room), or after a while.
   */
  settle(): Promise<void> {
    this.#take({ type: 'settle' });
    if (!this.#waitsForAnswers()) return Promise.resolve();
    return new Promise((resolve) => {
      const done = (): void => {
        clearTimeout(timer);
        this.#settling = this.#settling.filter((other) => other !== done);
        resolve();
      };
      const timer = setTimeout(done, SETTLE_WAIT);
      this.#settling.push(done);
    });
  }

  #waitsForAnswers(): boolean {
    const { phase, pending } = this.#room.status;
    return !this.#closed && pending && phase === 'joined';
  }

  close(): void {
    if (this.#closed) return;
    // What was gathered goes first: the server takes it in order, before the leaving.
    this.#take({ type: 'settle' });
    this.#closed = true;
    this.#port.close();
    for (const timer of this.#timers.values()) clearTimeout(timer);
    this.#timers.clear();
    this.#hold(undefined);
    for (const done of [...this.#settling]) done();
  }

  #isClosed(): boolean {
    return this.#closed;
  }

  #take(event: RoomEvent): void {
    if (this.#closed) return;
    const held = this.#doc;
    let changed = false;
    for (const effect of this.#room.take(event)) {
      // Closed by whoever was told something (`discarded`): the rest is not done.
      if (this.#isClosed()) return;
      if (effect.type === 'changed') changed = true;
      else this.#run(effect);
    }
    if (changed || held !== this.#doc) {
      this.#hears.state(this.state);
      if (!this.#waitsForAnswers()) for (const done of [...this.#settling]) done();
    }
  }

  #run(effect: Exclude<RoomEffect, { type: 'changed' }>): void {
    switch (effect.type) {
      case 'join':
        this.#port.join();
        break;
      case 'send':
        this.#port.send(encodeDocFrame(effect.frame));
        break;
      case 'apply':
        if (this.#doc) this.#apply(this.#doc, effect.update);
        break;
      case 'fresh': {
        const doc = new Y.Doc();
        this.#apply(doc, effect.update);
        this.#hold(doc);
        break;
      }
      case 'drop':
        this.#hold(undefined);
        break;
      case 'offer': {
        const doc = this.#doc;
        if (!doc) break;
        // Everything the server's lacks, and every deletion: one made here alone
        // shows in no state vector, so none can be told to be missing.
        const have = Y.encodeStateVectorFromUpdate(effect.have);
        this.#take({ type: 'missing', update: Y.encodeStateAsUpdate(doc, have) });
        break;
      }
      case 'discarded':
        this.#hears.discarded(effect.why);
        break;
      case 'timer':
        clearTimeout(this.#timers.get(effect.timer));
        this.#timers.set(
          effect.timer,
          setTimeout(() => {
            this.#timers.delete(effect.timer);
            this.#take({ type: 'timer', timer: effect.timer });
          }, effect.wait),
        );
        break;
      case 'cancel':
        clearTimeout(this.#timers.get(effect.timer));
        this.#timers.delete(effect.timer);
        break;
    }
  }

  /** Merges what the server sent; bytes that are no update are dropped (the server sends none). */
  #apply(doc: Y.Doc, update: Uint8Array): void {
    try {
      Y.applyUpdate(doc, update, this);
    } catch {
      // Nothing of it was taken in.
    }
  }

  readonly #changed = (update: Uint8Array, origin: unknown): void => {
    // What came from the room is not sent back to it.
    if (origin !== this) this.#take({ type: 'local', update });
  };

  /** Holds `doc` in place of the one before, which is let go. */
  #hold(doc: Y.Doc | undefined): void {
    const before = this.#doc;
    if (before === doc) return;
    before?.off('update', this.#changed);
    this.#doc = doc;
    doc?.on('update', this.#changed);
    before?.destroy();
  }
}

/** Opens a card's body on a way into its room. */
export function openRoom(line: RoomLine, hears: BodyDocHears): BodyDoc {
  return new RoomClient(line, hears);
}
