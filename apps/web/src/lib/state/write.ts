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

  busy(id: string): boolean {
    return this.#under.has(id);
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
