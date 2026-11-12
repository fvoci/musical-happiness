/**
 * The patches that take changes back, kept right while a write fails.
 *
 * A change shows at once and takes back to what is shown. Made while an
 * earlier write of the same thing is under way (a change, an undo, a redo),
 * it takes back to what that write shows, which is not stored yet. If that
 * write fails, it never was: the patch that takes the later change back is
 * made to take back to what is stored instead (what the failed write was to
 * replace).
 *
 * A patch is plain members and one map of values with the list of the keys
 * it clears (`updatedProperties` and `deletedProperties` of a card,
 * `updatedFields` and `deletedFields` of a view). Patches are changed in
 * place: the undo of a step holds the very object.
 */

type Patch = Record<string, unknown>;

interface Kept {
  id: string;
  /** How many writes had begun when it was made. */
  since: number;
  patch: Patch;
}

export class Inverses<P extends object> {
  readonly #updated: string;
  readonly #deleted: string;
  #begun = 0;
  #underWay = 0;
  #kept: Kept[] = [];

  constructor(updated: string, deleted: string) {
    this.#updated = updated;
    this.#deleted = deleted;
  }

  /** The patch that takes back a change of `id` being made now. */
  keep(id: string, patch: P): void {
    // With no write under way, no failure can come before it.
    if (this.#underWay > 0) this.#kept.push({ id, since: this.#begun, patch: patch as Patch });
  }

  /** Sends a write of `id` that shows `patch` over `reverse`; its failure corrects later patches. */
  async write<T>(id: string, patch: P, reverse: P, send: () => Promise<T>): Promise<T> {
    this.#begun += 1;
    const begun = this.#begun;
    this.#underWay += 1;
    try {
      return await send();
    } catch (error) {
      for (const kept of this.#kept) {
        if (kept.id === id && kept.since >= begun) {
          this.#rewrite(kept.patch, patch as Patch, reverse as Patch);
        }
      }
      throw error;
    } finally {
      this.#underWay -= 1;
      if (this.#underWay === 0) this.#kept = [];
    }
  }

  /** What in `later` takes back to what `failed` showed takes back to what it replaced. */
  #rewrite(later: Patch, failed: Patch, replaced: Patch): void {
    for (const [key, value] of Object.entries(failed)) {
      if (key === this.#updated) {
        for (const [name, shown] of Object.entries(value as Record<string, unknown>)) {
          if (same(this.#map(later)[name], shown)) this.#restore(later, name, replaced);
        }
      } else if (key === this.#deleted) {
        for (const name of value as string[]) {
          if (this.#list(later).includes(name)) this.#restore(later, name, replaced);
        }
      } else if (value !== undefined && same(later[key], value)) {
        later[key] = replaced[key];
      }
    }
  }

  /** Makes `later` take `name` back to what `replaced` says of it. */
  #restore(later: Patch, name: string, replaced: Patch): void {
    const values = { ...this.#map(later) };
    // eslint-disable-next-line @typescript-eslint/no-dynamic-delete -- a value is named by its key
    delete values[name];
    const cleared = this.#list(later).filter((other) => other !== name);
    const stored = this.#map(replaced);
    if (name in stored) values[name] = structuredClone(stored[name]);
    else if (this.#list(replaced).includes(name)) cleared.push(name);
    later[this.#updated] = values;
    later[this.#deleted] = cleared;
  }

  #map(patch: Patch): Record<string, unknown> {
    return (patch[this.#updated] as Record<string, unknown> | undefined) ?? {};
  }

  #list(patch: Patch): string[] {
    return (patch[this.#deleted] as string[] | undefined) ?? [];
  }
}

function same(first: unknown, second: unknown): boolean {
  return first === second || JSON.stringify(first) === JSON.stringify(second);
}
