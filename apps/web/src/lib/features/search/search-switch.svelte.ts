/**
 * Whether the search is there: unknown, and so off, until the backend says
 * (`searchEnabled`), and off for good once a search is answered that the
 * server has no search server (its settings changed). A backend that cannot
 * say leaves it off.
 */
import type { ContentSearch } from '$lib/state/content-search.ts';

export class SearchSwitch {
  on = $state(false);
  #gone = false;

  constructor(search: ContentSearch) {
    search.enabled().then(
      (enabled) => {
        if (!this.#gone) this.on = enabled;
      },
      () => undefined,
    );
  }

  /** The server has no search server any more. */
  turnOff(): void {
    this.#gone = true;
    this.on = false;
  }
}
