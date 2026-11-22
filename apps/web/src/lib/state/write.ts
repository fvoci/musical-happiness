/**
 * Keeping an optimistic copy steady while writes are under way.
 *
 * A store shows a change at once and then sends it. If two changes of the
 * same thing are sent one after the other, the answer to the first describes
 * the thing without the second change; showing it would take the second
 * change off the screen for a moment. So while writes of a thing are under
 * way, versions arriving from the backend (answers and change messages
 * alike) are held, and the newest of them is applied when the last write has
 * come back.
 */

interface Versioned {
  id: string;
  updateAt: number;
}

function newer<T extends Versioned>(first: T | undefined, second: T | undefined): T | undefined {
  if (!first) return second;
  if (!second) return first;
  return second.updateAt >= first.updateAt ? second : first;
}

export class PendingWrites<T extends Versioned> {
  readonly #under = new Map<string, number>();
  readonly #held = new Map<string, T>();

  /** A write of `id` is being sent. */
  begin(id: string): void {
    this.#under.set(id, (this.#under.get(id) ?? 0) + 1);
  }

  /**
   * A write of `id` came back, with `answer` if it succeeded. Returns the
   * version to show now, if this was the last write under way and the
   * backend said anything about the thing meanwhile.
   */
  end(id: string, answer?: T): T | undefined {
    const left = (this.#under.get(id) ?? 1) - 1;
    const latest = newer(this.#held.get(id), answer);
    if (left > 0) {
      this.#under.set(id, left);
      if (latest) this.#held.set(id, latest);
      return undefined;
    }
    this.#under.delete(id);
    this.#held.delete(id);
    return latest;
  }

  /** A version arrived from the backend. True: show it now. False: it is held. */
  receive(version: T): boolean {
    if (!this.#under.has(version.id)) return true;
    this.#held.set(version.id, newer(this.#held.get(version.id), version) ?? version);
    return false;
  }
}

/** `items` with `item` put in place of the one with its identifier, or added. */
export function put<T extends { id: string }>(items: readonly T[], item: T): T[] {
  const at = items.findIndex((existing) => existing.id === item.id);
  if (at === -1) return [...items, item];
  const next = [...items];
  next[at] = item;
  return next;
}

/** `items` with a version from the backend taken in, unless what is there is newer. */
export function take<T extends Versioned>(items: readonly T[], version: T): T[] {
  const existing = items.find((item) => item.id === version.id);
  if (existing && existing.updateAt > version.updateAt) return items as T[];
  return put(items, version);
}

/**
 * A list read again over the one shown, after changes may have been missed:
 * what was read, except where what is shown is newer, or a write under way
 * holds it (`hold` false: keep what is shown); and what arrived while the
 * list was being read. What was shown when the reading began (`before`) and
 * is not in what was read is gone; so is what was shown then and was taken
 * off meanwhile (a deletion arrived while reading), even if the reading,
 * older, still has it.
 */
export function reread<T extends Versioned>(
  shown: readonly T[],
  read: readonly T[],
  before: readonly string[],
  hold: (item: T) => boolean = () => true,
): T[] {
  const wasRead = new Set(read.map((item) => item.id));
  const wasShown = new Set(before);
  const next: T[] = [];
  for (const item of read) {
    const current = shown.find((other) => other.id === item.id);
    if (!current && wasShown.has(item.id)) continue;
    if (!hold(item)) next.push(current ?? item);
    else next.push(current && current.updateAt > item.updateAt ? current : item);
  }
  for (const item of shown) {
    if (!wasRead.has(item.id) && !wasShown.has(item.id)) next.push(item);
  }
  return next;
}

/**
 * Runs writes of one block in turn so two quick ones do not pass each other,
 * and advances `readAt` along queued view writes from each succeeded write's
 * returned `updateAt` so a write does not get 409 from the client's own
 * earlier write in the queue.
 */
export class BlockTurns {
  readonly #writes = new Map<string, Promise<void>>();
  readonly #advanced = new Map<string, Map<number, number>>();

  /** Runs `write` once earlier writes of `blockId` have ended, however they ended. */
  run<T>(blockId: string, write: () => Promise<T>): Promise<T> {
    const before = this.#writes.get(blockId) ?? Promise.resolve();
    const running = before.then(write);
    const settled = running.then(
      () => undefined,
      () => undefined,
    );
    this.#writes.set(blockId, settled);
    void settled.then(() => {
      if (this.#writes.get(blockId) === settled) {
        this.#writes.delete(blockId);
        this.#advanced.delete(blockId);
      }
    });
    return running;
  }

  /**
   * Runs a view patch in turn for `viewId`, forwarding `patch.readAt` along
   * this queue's succeeded writes when it matches an earlier write's base
   * `updateAt`. A failed write, or one whose returned `order` differs from
   * what was written, does not advance the chain.
   */
  patchView<
    T extends { updateAt: number; fields: { order?: string } },
    P extends { readAt?: number; updatedFields?: Record<string, unknown> },
  >(
    viewId: string,
    patch: P,
    send: (sent: P) => Promise<{ before: number; after: T }>,
  ): Promise<T> {
    return this.run(viewId, async () => {
      const readAt = patch.readAt === undefined ? undefined : this.#resolve(viewId, patch.readAt);
      const sent = readAt === patch.readAt ? patch : { ...patch, readAt };
      const { before, after } = await send(sent);
      const from = sent.readAt ?? before;
      const orderKept =
        !sent.updatedFields ||
        !('order' in sent.updatedFields) ||
        after.fields.order === sent.updatedFields.order;
      if (orderKept && after.updateAt > from) {
        let map = this.#advanced.get(viewId);
        if (!map) {
          map = new Map();
          this.#advanced.set(viewId, map);
        }
        map.set(from, after.updateAt);
      }
      return after;
    });
  }

  #resolve(viewId: string, readAt: number): number {
    const map = this.#advanced.get(viewId);
    if (!map) return readAt;
    let current = readAt;
    while (map.has(current)) current = map.get(current) ?? current;
    return current;
  }
}
