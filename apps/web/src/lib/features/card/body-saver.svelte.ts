/**
 * When the body of an open card is stored.
 *
 * The card store keeps what the reader wrote and writes it when told
 * (`CardStore.saveBody`). This decides when: a pause in the typing, the
 * focus leaving the editor, the card being closed or changed for another.
 * It also says, for the line at the top of the card, how the saving stands.
 */
import type { DocNode } from '$lib/editor/schema.ts';
import type { CardStore } from '$lib/state/index.ts';

/** How long the typing has to pause before the body is stored, in milliseconds. */
export const SAVE_PAUSE = 800;

/**
 * - `idle`: nothing was written in this visit.
 * - `saving`: a write is under way, or waits for the typing to pause.
 * - `saved`: everything written in this visit is stored.
 * - `unsaved`: a write failed; the text is still here and goes with the next one.
 * - `conflict`: somebody else wrote the body; the reader has to choose.
 */
export type SaveState = 'idle' | 'saving' | 'saved' | 'unsaved' | 'conflict';

/** A value as text that is the same whatever order the members of its objects were written in. */
function canonical(value: unknown): string {
  if (value === undefined) return 'null';
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (typeof value !== 'object' || value === null) return JSON.stringify(value);
  const record = value as Record<string, unknown>;
  const members = Object.keys(record)
    .filter((key) => record[key] !== undefined)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${canonical(record[key])}`);
  return `{${members.join(',')}}`;
}

/** Whether two bodies say the same. An editor writes the members of a node in its own order. */
export const sameBody = (first: DocNode | null, second: DocNode | null): boolean =>
  canonical(first) === canonical(second);

export class BodySaver {
  readonly card: CardStore;
  readonly #pause: number;
  #timer: ReturnType<typeof setTimeout> | undefined;
  /** The newest body the editor reported, and the revision of the stored body it was an edit of. */
  #latest: { body: DocNode | null; revision: number } | undefined;
  /** A write waits for the typing to pause. */
  #waiting = $state(false);
  /** Something was stored in this visit. */
  #stored = $state(false);

  readonly state = $derived.by<SaveState>(() => {
    if (this.card.conflict) return 'conflict';
    if (this.card.saving || this.#waiting) return 'saving';
    if (this.card.dirty) return 'unsaved';
    return this.#stored ? 'saved' : 'idle';
  });

  constructor(card: CardStore, pause: number = SAVE_PAUSE) {
    this.card = card;
    this.#pause = pause;
  }

  /** What the editor holds, as far as this knows: the last body it reported, or else the stored one. */
  #shown(): DocNode | null {
    const latest = this.#latest;
    return latest?.revision === this.card.bodyRevision ? latest.body : this.card.body;
  }

  /**
   * The editor reports its body. When that is not what it held before, the
   * reader changed it: it is remembered, and stored once the typing pauses.
   * An editor also reports without a change (when it is made, when it is
   * switched between editing and reading); that is nothing to store.
   */
  edited(body: DocNode | null): void {
    if (sameBody(this.#shown(), body)) return;
    this.#latest = { body, revision: this.card.bodyRevision };
    this.card.editBody(body);
    clearTimeout(this.#timer);
    this.#waiting = true;
    this.#timer = setTimeout(() => {
      void this.flush();
    }, this.#pause);
  }

  /** Stores what is unsaved now, without waiting for a pause. */
  async flush(): Promise<void> {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#waiting = false;
    if (this.#clean()) return;
    await this.card.saveBody();
    if (this.#clean()) this.#stored = true;
  }

  /** Nothing the reader wrote is waiting to be stored. Asked again after every write. */
  #clean(): boolean {
    return !this.card.dirty;
  }

  /**
   * Settles a conflict in favour of what the reader wrote. That is the
   * editor's text as it is now: what was typed between the refused write and
   * the notice is part of it.
   */
  async keepMine(): Promise<void> {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#waiting = false;
    const latest = this.#latest;
    await this.card.resolveConflict('mine');
    if (latest && !this.card.conflict && !sameBody(latest.body, this.card.body)) {
      await this.card.saveBody(latest.body);
    }
    if (this.#clean()) this.#stored = true;
  }

  /** Settles a conflict in favour of what is stored; the editor takes that text. */
  async takeTheirs(): Promise<void> {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#waiting = false;
    this.#latest = undefined;
    await this.card.resolveConflict('theirs');
  }

  /** Stops the clock. What is unsaved is not stored by this: call `flush` first. */
  dispose(): void {
    clearTimeout(this.#timer);
    this.#timer = undefined;
  }
}
