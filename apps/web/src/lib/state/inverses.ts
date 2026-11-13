/**
 * Inverse patches follow the writes their values came from. A refusal only
 * corrects dependent values: an equal value written successfully later has
 * its own origin. Definitions in a board's list are tracked by identifier;
 * a definition remains one value (its type and options belong together).
 */
import { InverseValues, sameValue } from './inverse-values.ts';

type Patch = Record<string, unknown>;

interface Origin {
  value: unknown;
  beforeValue: unknown;
  before: Origin | undefined;
  state: 'pending' | 'stored' | 'refused';
}

interface Kept {
  patch: Patch;
  origins: Map<string, Origin>;
}

/** The nearest value that was not refused, and its origin if it can still fail. */
function surviving(origin: Origin): { value: unknown; origin?: Origin } {
  let current = origin;
  while (current.state === 'refused') {
    if (!current.before) return { value: current.beforeValue };
    current = current.before;
  }
  return { value: current.value, origin: current.state === 'pending' ? current : undefined };
}

export class Inverses<P extends object> {
  readonly #values: InverseValues;
  readonly #shown = new Map<string, Map<string, Origin>>();
  readonly #kept = new Set<Kept>();

  constructor(updated: string, deleted: string) {
    this.#values = new InverseValues(updated, deleted);
  }

  /** Keeps only the values whose currently shown source can still fail. */
  keep(id: string, patch: P): void {
    const shown = this.#shown.get(id);
    if (!shown) return;
    const origins = new Map<string, Origin>();
    for (const [key, value] of this.#values.read(patch as Patch)) {
      const origin = shown.get(key);
      if (origin && sameValue(origin.value, value)) origins.set(key, origin);
    }
    if (origins.size > 0) this.#kept.add({ patch: patch as Patch, origins });
  }

  /** Sends a write; a refusal corrects only the inverse values descended from that write. */
  async write<T>(
    id: string,
    patch: P,
    reverse: P,
    send: () => Promise<T>,
    refused?: (restore: P) => void,
  ): Promise<T> {
    const origins = this.#begin(id, patch, reverse);
    try {
      const answer = await send();
      for (const origin of origins.values()) {
        origin.state = 'stored';
        // A successful value does not depend on what came before it any more.
        origin.before = undefined;
      }
      return answer;
    } catch (error) {
      for (const origin of origins.values()) origin.state = 'refused';
      this.#correct();
      const restore: Patch = {};
      for (const [key, origin] of origins) {
        if (this.#shown.get(id)?.get(key) === origin) {
          this.#values.put(restore, key, surviving(origin).value);
        }
      }
      refused?.(restore as P);
      throw error;
    } finally {
      this.#release(id);
    }
  }

  #begin(id: string, patch: P, reverse: P): Map<string, Origin> {
    const shown = this.#shown.get(id) ?? new Map<string, Origin>();
    const before = this.#values.read(reverse as Patch);
    const origins = new Map<string, Origin>();
    for (const [key, value] of this.#values.read(patch as Patch)) {
      const beforeValue = before.get(key);
      const previous = shown.get(key);
      const origin: Origin = {
        value,
        beforeValue,
        before: previous && sameValue(previous.value, beforeValue) ? previous : undefined,
        state: 'pending',
      };
      origins.set(key, origin);
      shown.set(key, origin);
    }
    if (shown.size > 0) this.#shown.set(id, shown);
    return origins;
  }

  #correct(): void {
    for (const kept of this.#kept) {
      const values = this.#values.read(kept.patch);
      for (const [key, origin] of kept.origins) {
        if (origin.state !== 'refused') continue;
        const replacement = surviving(origin);
        if (sameValue(values.get(key), origin.value)) {
          this.#values.put(kept.patch, key, replacement.value);
        }
        if (replacement.origin) kept.origins.set(key, replacement.origin);
        else kept.origins.delete(key);
      }
    }
  }

  /** Unrelated writes cannot keep these references alive. */
  #release(id: string): void {
    const shown = this.#shown.get(id);
    for (const [key, origin] of shown ?? []) {
      const replacement = surviving(origin).origin;
      if (replacement) shown?.set(key, replacement);
      else shown?.delete(key);
    }
    if (shown?.size === 0) this.#shown.delete(id);
    for (const kept of this.#kept) {
      for (const [key, origin] of kept.origins) {
        if (origin.state !== 'pending') kept.origins.delete(key);
      }
      if (kept.origins.size === 0) this.#kept.delete(kept);
    }
  }
}
