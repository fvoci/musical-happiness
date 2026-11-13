/** Records and promise helpers shared by the undo history. */
export interface Step {
  undo: (value?: unknown) => Promise<void>;
  redo: () => Promise<unknown>;
  description: string | undefined;
  value: unknown;
}

export interface Entry {
  /** What has actually begun, independently of queued requests. */
  inForce: boolean;
  /** The requests preceding a reservation; undefined once it changes something. */
  reservedAt: number | undefined;
  /** Redo entries hidden provisionally while a reservation may still change nothing. */
  hidden: Entry[];
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
export interface Gesture {
  over: Promise<void>;
  /** A caller must not open a result that was already asked to be taken back. */
  undoRequested: boolean;
  /** The step its next change joins. */
  entry: Entry | undefined;
  /** Every step it made: more than one where a change of the person came between its own. */
  entries: Entry[];
  /** The history it was begun in (`clear` begins another). */
  history: number;
}

export type Action = 'undo' | 'redo';

export interface Asked {
  number: number;
  entry: Entry;
  action: Action;
  cancelled: boolean;
  dropped: ReturnType<typeof deferred>;
}

/** A step in no history. */
export function detached(groupId: string | undefined): Entry {
  return {
    inForce: true,
    reservedAt: undefined,
    hidden: [],
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
export function replaceCut(holder: Entry, entry: Entry, by: Entry | undefined): boolean {
  const at = holder.cut.indexOf(entry);
  if (at !== -1) {
    if (by) holder.cut[at] = by;
    else holder.cut.splice(at, 1);
    return true;
  }
  return holder.cut.some((other) => replaceCut(other, entry, by));
}

/** Calls `run` now, turning a throw into a rejection. */
export async function call<T>(run: () => Promise<T>): Promise<T> {
  return run();
}

export function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve = (): void => undefined;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/** Waits until the gesture of `entry` is over and its writes, and an undo or redo of it, are done. */
export async function settled(entry: Entry): Promise<void> {
  while (entry.gesture || entry.making.size > 0 || entry.carrying) {
    await Promise.allSettled([entry.gesture?.over, entry.carrying, ...entry.making]);
  }
}
