/**
 * The search of the sidebar asked of the backend as well: what it finds
 * that the sidebar does not show (a hidden board; a title that has the
 * words in another order). Asked a moment after the typing stops; of two
 * answers, only the one to the latest query counts.
 */
import type { Board } from '$lib/logic/wire.ts';

/** How long the typing has to stop before the backend is asked, in milliseconds. */
const PAUSE = 250;

export class BoardSearch {
  found = $state.raw<Board[]>([]);
  readonly #search: (query: string) => Promise<Board[]>;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #asked = 0;

  constructor(search: (query: string) => Promise<Board[]>) {
    this.#search = search;
  }

  /** The query as it is now; an empty one finds nothing at once. */
  run(query: string): void {
    clearTimeout(this.#timer);
    this.#asked += 1;
    const asked = this.#asked;
    if (query.trim() === '') {
      this.found = [];
      return;
    }
    this.#timer = setTimeout(() => {
      void this.#search(query).then((found) => {
        if (asked === this.#asked) this.found = found;
      });
    }, PAUSE);
  }

  stop(): void {
    clearTimeout(this.#timer);
    this.#asked += 1;
  }
}
