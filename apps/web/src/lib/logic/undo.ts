/**
 * Undo and redo of the changes a person makes.
 *
 * A change is a pair of functions: `redo` performs it and `undo` takes it
 * back. Whatever `redo` returns is handed to `undo`, so a change that creates
 * something (and learns its identifier only from the server's answer) can
 * remove exactly that.
 *
 * A change is a step of the history the moment it is made, not when the
 * backend has answered: undo always takes back what the person did last.
 * Undo and redo are asked for at once (what can be undone next moves at
 * once) and carried out one after the other, each when the writes of its
 * step have come back. A change the backend refuses never happened: it
 * leaves the history, and an undo or redo that was waiting for it is
 * dropped without touching the step before it.
 *
 * A change is made at once, also while undos or redos wait. It was made on
 * the screen as it was, which did not show them yet: they are dropped, and
 * the history goes on from what is in force. One that is being carried out
 * shows already, and stays.
 *
 * Changes that share a `groupId` are one step. A gesture whose changes are
 * made one after the other (each waiting for the one before) runs inside
 * `group`: its step is in place from its first change, and an undo asked
 * for meanwhile waits for the end of the gesture.
 *
 * `redo` and `undo` must not perform changes themselves.
 */

interface Step {
  undo: (value?: unknown) => Promise<void>;
  redo: () => Promise<unknown>;
  description: string | undefined;
  value: unknown;
}

interface Entry {
  checkpoint: number;
  groupId: string | undefined;
  /** In the order they were made. */
  steps: Step[];
  /** The writes of changes of this step that have not come back. */
  making: Set<Promise<unknown>>;
  /** The undo or redo of this step being carried out. */
  carrying: Promise<unknown> | undefined;
  /** The gesture of a `group`, while it is under way. */
  gesture: Gesture | undefined;
  /** What could be redone when this step was made: back if it ends with no change. */
  cut: Entry[];
}

/** A gesture under way, and its step once it has made a change. */
interface Gesture {
  over: Promise<void>;
  /** The step its next change joins. */
  entry: Entry | undefined;
  /** Every step it made: more than one where a change of the person came between its own. */
  entries: Entry[];
  /** The history it was begun in (`clear` begins another). */
  history: number;
}

type Action = 'undo' | 'redo';

let groupCounter = 0;

export class UndoManager {
  /** Called whenever what can be undone or redone, or `busy`, has changed. */
  onStateDidChange?: () => void;

  #entries: Entry[] = [];
  /** The step undo takes back next, as asked for. */
  #index = -1;
  /** The same, as far as undos and redos have begun: the last step in force. */
  #inForce = -1;
  /** Gestures still under way, by group. */
  readonly #gestures = new Map<string, Gesture>();
  /** The turn the next undo or redo waits for. */
  #queue: Promise<unknown> = Promise.resolve();
  /** The undo or redo being carried out: the next one begins after it, also once the queue let go. */
  #carried: Promise<unknown> = Promise.resolve();
  /** Undos and redos asked for and not yet over. */
  #waiting = 0;
  /** Raised when what is asked for is dropped: by a new change, a failure, `clear`. */
  #generation = 0;
  /** Settles when the asks of this generation are dropped. */
  #dropped = deferred();
  /** Times the history was cleared. */
  #history = 0;

  /** When the change that would be undone next was made; 0 without one. */
  get currentCheckpoint(): number {
    return this.#entries[this.#index]?.checkpoint ?? 0;
  }

  get undoDescription(): string | undefined {
    return this.#entries[this.#index]?.steps.at(-1)?.description;
  }

  get redoDescription(): string | undefined {
    return this.#entries[this.#index + 1]?.steps[0]?.description;
  }

  get canUndo(): boolean {
    return this.#index !== -1;
  }

  get canRedo(): boolean {
    return this.#index < this.#entries.length - 1;
  }

  /** Whether an undo or redo is waiting for a write, or being carried out. */
  get busy(): boolean {
    return this.#waiting > 0;
  }

  /**
   * Performs a change and remembers how to take it back: the step is in place
   * at once. Returns what `redo` returned; rejects, and forgets the change,
   * when `redo` rejects. A discardable change takes the checkpoint of the
   * change before it instead of the current time (0 when it is the first).
   */
  async perform<T>(
    redo: () => Promise<T>,
    undo: (value: T) => Promise<void>,
    description?: string,
    groupId?: string,
    isDiscardable = false,
  ): Promise<T> {
    const step: Step = {
      undo: undo as (value?: unknown) => Promise<void>,
      redo,
      description,
      value: undefined,
    };
    const entry = this.#place(step, groupId, isDiscardable);
    const making = call(redo);
    entry.making.add(making);
    try {
      const value = await making;
      step.value = value;
      return value;
    } catch (error) {
      entry.steps = entry.steps.filter((other) => other !== step);
      this.#forgetIfEmpty(entry);
      throw error;
    } finally {
      entry.making.delete(making);
    }
  }

  /**
   * Runs a gesture whose changes (performed with the `groupId` it is given)
   * are one step, in place from the first of them. An undo asked for while
   * the gesture is under way waits for its end. A gesture that changes
   * nothing leaves the history as it was.
   */
  async group<T>(gesture: (groupId: string) => Promise<T>): Promise<T> {
    groupCounter += 1;
    const groupId = `group-${groupCounter}`;
    const over = deferred();
    const record: Gesture = {
      over: over.promise,
      entry: undefined,
      entries: [],
      history: this.#history,
    };
    this.#gestures.set(groupId, record);
    try {
      return await gesture(groupId);
    } finally {
      this.#gestures.delete(groupId);
      over.resolve();
      for (const entry of record.entries) {
        entry.gesture = undefined;
        this.#forgetIfEmpty(entry);
      }
    }
  }

  /**
   * Takes back the last step that is not undone. What can be undone next
   * moves at once; the step is taken back when its writes have come back.
   * Rejects when taking it back fails (it then stays the one to undo).
   */
  async undo(): Promise<this> {
    const entry = this.#entries[this.#index];
    if (!entry) return this;
    this.#index -= 1;
    await this.#ask(entry, 'undo');
    return this;
  }

  /** Does again the step undone last; as `undo`, the other way. */
  async redo(): Promise<this> {
    const entry = this.#entries[this.#index + 1];
    if (!entry) return this;
    this.#index += 1;
    await this.#ask(entry, 'redo');
    return this;
  }

  /**
   * Forgets every step. Undos and redos still waiting are dropped; changes,
   * undos and redos still under way end without touching the new history
   * (their steps are no longer in it), and a gesture begun before is not
   * remembered.
   */
  clear(): void {
    this.#history += 1;
    // What is still carried out is of the history left: the new one waits for none of it.
    this.#carried = Promise.resolve();
    this.#waiting = 0;
    const hadEntries = this.#entries.length > 0;
    this.#entries = [];
    this.#index = -1;
    this.#inForce = -1;
    this.#drop();
    if (hadEntries) this.onStateDidChange?.();
  }

  /** The step `step` belongs to: its gesture's, the last step's of its group, or a new one. */
  #place(step: Step, groupId: string | undefined, isDiscardable: boolean): Entry {
    const gesture = groupId === undefined ? undefined : this.#gestures.get(groupId);
    const last = this.#entries[this.#index];
    let entry = gesture
      ? this.#continued(gesture)
      : groupId !== undefined && last?.groupId === groupId
        ? last
        : undefined;
    if (!entry) {
      // A gesture begun in a history since cleared is of that history: its step is in none.
      entry =
        gesture && gesture.history !== this.#history
          ? detached(groupId)
          : this.#push(groupId, isDiscardable);
      if (gesture) {
        gesture.entry = entry;
        gesture.entries.push(entry);
        entry.gesture = gesture;
      }
    }
    entry.steps.push(step);
    this.onStateDidChange?.();
    return entry;
  }

  /**
   * The step a gesture's next change joins: its own, unless a change of the
   * person was made after it and no undo or redo waits. That change came between, so the gesture goes
   * on in a new step after it: undo and redo then go in the order the
   * changes were made.
   */
  #continued(gesture: Gesture): Entry | undefined {
    const { entry } = gesture;
    // Undos asked for meanwhile were asked of the gesture as one step: it stays one, so
    // that they take all of it back (a new step would drop them, as a new change does).
    if (this.#waiting > 0) return entry;
    const at = entry ? this.#entries.indexOf(entry) : -1;
    return at !== -1 && at < this.#entries.length - 1 ? undefined : entry;
  }

  /**
   * A new step after the last one in force. Undos and redos that wait are
   * dropped; what could be redone is gone (kept in `cut`).
   */
  #push(groupId: string | undefined, isDiscardable: boolean): Entry {
    if (this.#waiting > 0) {
      this.#drop();
      this.#index = this.#inForce;
    }
    const cut = this.#entries.splice(this.#index + 1);
    const checkpoint = isDiscardable ? (this.#entries.at(-1)?.checkpoint ?? 0) : Date.now();
    const entry: Entry = { ...detached(groupId), checkpoint, cut };
    this.#entries.push(entry);
    this.#index = this.#entries.length - 1;
    this.#inForce = this.#index;
    return entry;
  }

  /**
   * A step left without changes (all refused, or a gesture that changed
   * nothing) leaves the history. When it is still the last step, what could
   * be redone before it was made comes back.
   */
  #forgetIfEmpty(entry: Entry): void {
    if (entry.steps.length > 0 || entry.gesture) return;
    const at = this.#entries.indexOf(entry);
    if (at === -1) return;
    this.#entries.splice(at, 1);
    // What it cut comes back once nothing made after it stays: the step after it keeps it until then.
    const next = this.#entries[at];
    if (!next) this.#entries.push(...entry.cut.filter((other) => other.steps.length > 0));
    else if (next.cut.length === 0) next.cut = entry.cut;
    if (at <= this.#index) this.#index -= 1;
    if (at <= this.#inForce) this.#inForce -= 1;
    this.onStateDidChange?.();
  }

  /** Drops the undos and redos asked for and not yet begun. */
  #drop(): void {
    this.#generation += 1;
    this.#dropped.resolve();
    this.#dropped = deferred();
  }

  /** Queues an undo or redo of `entry`, and waits for it to be over. */
  async #ask(entry: Entry, action: Action): Promise<void> {
    const generation = this.#generation;
    const dropped = this.#dropped.promise;
    const current = (): boolean => generation === this.#generation;
    const history = this.#history;
    this.#waiting += 1;
    this.onStateDidChange?.();
    const turn = this.#queue.then(async () => {
      if (!current()) return;
      // Waits for the writes of the step, or for being dropped (a write may never end).
      await Promise.race([settled(entry), dropped]);
      if (!current()) return;
      // One undo or redo at a time, in the order asked for: two writes of one thing never cross.
      await this.#carried;
      const at = this.#entries.indexOf(entry);
      // Dropped, or every change of the step was refused.
      if (!current() || at === -1) return;
      this.#inForce = action === 'undo' ? at - 1 : at;
      const carrying = this.#carryOut(entry, action);
      entry.carrying = carrying.catch(() => undefined);
      this.#carried = entry.carrying;
      try {
        await carrying;
      } finally {
        entry.carrying = undefined;
      }
    });
    // The next waits for this one, unless what is asked for is dropped meanwhile.
    this.#queue = Promise.race([turn.catch(() => undefined), dropped]);
    try {
      await turn;
    } finally {
      // One of a history since cleared was let go of by `clear`.
      if (history === this.#history) this.#waiting -= 1;
      this.onStateDidChange?.();
    }
  }

  async #carryOut(entry: Entry, action: Action): Promise<void> {
    const { steps } = entry;
    if (action === 'undo') {
      for (const [at, step] of [...steps.entries()].reverse()) {
        try {
          await step.undo(step.value);
        } catch (error) {
          this.#failed(entry, at + 1);
          throw error;
        }
      }
      return;
    }
    for (const [at, step] of steps.entries()) {
      try {
        step.value = await step.redo();
      } catch (error) {
        this.#failed(entry, at);
        throw error;
      }
    }
  }

  /**
   * An undo or redo of `entry` failed with its first `done` changes in force
   * and the rest not. The step is split there and the history made to say
   * what is in force; the undos and redos asked for after it are dropped,
   * unless the step was undone and a newer change has taken its place.
   */
  #failed(entry: Entry, done: number): void {
    const piece = (steps: Step[]): Entry => ({
      ...entry,
      steps,
      making: new Set(),
      carrying: undefined,
      gesture: undefined,
      cut: [],
    });
    const inForce = piece(entry.steps.slice(0, done));
    const notInForce = piece(entry.steps.slice(done));
    const at = this.#entries.indexOf(entry);
    if (at === -1) {
      // A change made while it was undone took its place (perhaps in what it cut in
      // turn): what is still in force goes before the step of the history that holds it.
      if (inForce.steps.length === 0) return;
      const rest = notInForce.steps.length > 0 ? notInForce : undefined;
      const host = this.#entries.findIndex((other) => replaceCut(other, entry, rest));
      if (host === -1) return;
      this.#entries.splice(host, 0, inForce);
      if (host <= this.#inForce) this.#inForce += 1;
      // Undos asked for down to the newer change still hold; one asked for below it did not
      // know that this step stays.
      if (this.#index >= host - 1) {
        this.#index += 1;
        this.onStateDidChange?.();
        return;
      }
    } else if (this.#inForce > at) {
      // A change made while it was redone comes after it: what is not in force is gone.
      this.#entries.splice(at, 1, ...(inForce.steps.length > 0 ? [inForce] : []));
      if (inForce.steps.length === 0) this.#inForce -= 1;
    } else {
      const pieces = [inForce, notInForce].filter((piece) => piece.steps.length > 0);
      this.#entries.splice(at, 1, ...pieces);
      this.#inForce = inForce.steps.length > 0 ? at : at - 1;
    }
    this.#index = this.#inForce;
    this.#drop();
    this.onStateDidChange?.();
  }
}

/** A step in no history. */
function detached(groupId: string | undefined): Entry {
  return {
    checkpoint: 0,
    groupId,
    steps: [],
    making: new Set(),
    carrying: undefined,
    gesture: undefined,
    cut: [],
  };
}

/**
 * Puts `by` in place of `entry` in what `holder` cut, however deep (takes it
 * out without `by`: nothing of it can be redone); whether it was there.
 */
function replaceCut(holder: Entry, entry: Entry, by: Entry | undefined): boolean {
  const at = holder.cut.indexOf(entry);
  if (at !== -1) {
    if (by) holder.cut[at] = by;
    else holder.cut.splice(at, 1);
    return true;
  }
  return holder.cut.some((other) => replaceCut(other, entry, by));
}

/** Calls `run` now, turning a throw into a rejection. */
async function call<T>(run: () => Promise<T>): Promise<T> {
  return run();
}

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve = (): void => undefined;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/** Waits until the gesture of `entry` is over and its writes, and an undo or redo of it, are done. */
async function settled(entry: Entry): Promise<void> {
  while (entry.gesture || entry.making.size > 0 || entry.carrying) {
    await Promise.allSettled([entry.gesture?.over, entry.carrying, ...entry.making]);
  }
}
