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
 * the history goes on from what is in force.
 *
 * Changes that share a `groupId` are one step. A gesture whose changes are
 * made one after the other (each waiting for the one before) runs inside
 * `group`, so that its step is in place from the start and an undo asked
 * for meanwhile waits for all of it.
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
  /** Settles when the gesture of a `group` is over; settled otherwise. */
  closed: Promise<void>;
  open: boolean;
  /** What could be redone when this step was made: back if it ends with no change. */
  cut: Entry[];
}

type Action = 'undo' | 'redo';

let groupCounter = 0;

export class UndoManager {
  /** Called whenever what can be undone or redone, or `busy`, has changed. */
  onStateDidChange?: () => void;

  #entries: Entry[] = [];
  /** The step undo takes back next, as asked for. */
  #index = -1;
  /** The same, as far as undos and redos have been begun: the last step in force. */
  #inForce = -1;
  /** Groups whose gesture is still under way, by identifier. */
  readonly #open = new Map<string, Entry>();
  /** Undo and redo carried out one after the other. */
  #queue: Promise<void> = Promise.resolve();
  /** Undos and redos asked for and not yet over. */
  #waiting = 0;
  /** Raised when what is asked for is dropped: by a new change, a failure, `clear`. */
  #generation = 0;
  /** Settles when the asks of this generation are dropped. */
  #dropped = deferred();

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
   * are one step. The step is in place before the first change is made; an
   * undo asked for while the gesture is under way waits for its end.
   */
  async group<T>(gesture: (groupId: string) => Promise<T>): Promise<T> {
    groupCounter += 1;
    const groupId = `group-${groupCounter}`;
    const closed = deferred();
    const entry = this.#push(groupId, false, closed.promise);
    entry.open = true;
    this.#open.set(groupId, entry);
    try {
      return await gesture(groupId);
    } finally {
      entry.open = false;
      this.#open.delete(groupId);
      closed.resolve();
      this.#forgetIfEmpty(entry);
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

  /** Forgets every step. Undos and redos still waiting are dropped. */
  clear(): void {
    const hadEntries = this.#entries.length > 0;
    this.#entries = [];
    this.#index = -1;
    this.#inForce = -1;
    this.#drop();
    if (hadEntries) this.onStateDidChange?.();
  }

  /** The step `step` belongs to: an open group's, the last step's of its group, or a new one. */
  #place(step: Step, groupId: string | undefined, isDiscardable: boolean): Entry {
    const open = groupId === undefined ? undefined : this.#open.get(groupId);
    const last = this.#entries[this.#index];
    const joined = open ?? (groupId !== undefined && last?.groupId === groupId ? last : undefined);
    const entry = joined ?? this.#push(groupId, isDiscardable, Promise.resolve());
    entry.steps.push(step);
    return entry;
  }

  /**
   * A new step after the last one in force. Undos and redos that wait are
   * dropped; what could be redone is gone (kept in `cut`).
   */
  #push(groupId: string | undefined, isDiscardable: boolean, closed: Promise<void>): Entry {
    if (this.#waiting > 0) {
      this.#drop();
      this.#index = this.#inForce;
    }
    const cut = this.#entries.splice(this.#index + 1);
    const checkpoint = isDiscardable ? (this.#entries.at(-1)?.checkpoint ?? 0) : Date.now();
    const entry: Entry = {
      checkpoint,
      groupId,
      steps: [],
      making: new Set(),
      closed,
      open: false,
      cut,
    };
    this.#entries.push(entry);
    this.#index = this.#entries.length - 1;
    this.#inForce = this.#index;
    this.onStateDidChange?.();
    return entry;
  }

  /**
   * A step left without changes (all refused, or a gesture that changed
   * nothing) leaves the history. When it is still the last step, what could
   * be redone before it was made comes back.
   */
  #forgetIfEmpty(entry: Entry): void {
    if (entry.steps.length > 0 || entry.open) return;
    const at = this.#entries.indexOf(entry);
    if (at === -1) return;
    this.#entries.splice(at, 1);
    if (at === this.#entries.length) this.#entries.push(...entry.cut);
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
    this.#waiting += 1;
    this.onStateDidChange?.();
    const turn = this.#queue.then(async () => {
      if (!current()) return;
      // Waits for the writes of the step, or for being dropped (a write that never ends).
      await Promise.race([settled(entry), dropped]);
      const at = this.#entries.indexOf(entry);
      // Dropped, or every change of the step was refused.
      if (!current() || at === -1) return;
      this.#inForce = action === 'undo' ? at - 1 : at;
      await this.#carryOut(entry, action);
    });
    this.#queue = turn.catch(() => undefined);
    try {
      await turn;
    } finally {
      this.#waiting -= 1;
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
   * and the rest not: the step is split there, the history says what is in
   * force, and the undos and redos asked for after it are dropped.
   */
  #failed(entry: Entry, done: number): void {
    const at = this.#entries.indexOf(entry);
    // A new change was made meanwhile: the step is no longer in the history.
    if (at === -1) return;
    this.#drop();
    const inForce = { ...entry, steps: entry.steps.slice(0, done), cut: [] };
    const notInForce = { ...entry, steps: entry.steps.slice(done), cut: [] };
    const pieces = [inForce, notInForce].filter((piece) => piece.steps.length > 0);
    this.#entries.splice(at, 1, ...pieces);
    this.#index = inForce.steps.length > 0 ? at : at - 1;
    this.#inForce = this.#index;
    this.onStateDidChange?.();
  }
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

/** Waits until the gesture of `entry` is over and its writes have come back. */
async function settled(entry: Entry): Promise<void> {
  while (entry.open || entry.making.size > 0) {
    await Promise.allSettled([entry.closed, ...entry.making]);
  }
}
