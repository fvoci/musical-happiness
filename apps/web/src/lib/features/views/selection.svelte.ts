/**
 * The cards selected in a view of a board, to be changed, moved or deleted
 * together: the kanban, the table and the gallery have one each, and the bar
 * that acts on it (BulkBar.svelte) is the same for all of them.
 *
 * With a mouse a card is taken in or out with Ctrl or ⌘ and a range with
 * Shift. A finger has neither: "Select" in the menu of a card turns on a
 * state in which a plain press selects (`selecting`), until the selection is
 * empty again. Escape lets it go.
 *
 * No more than `CARD_BATCH_LIMIT` cards are selected: what one request of
 * the board's batch routes takes. A press that would take in more takes in
 * what fits, and `full` says so until the selection changes again.
 */
import { CARD_BATCH_LIMIT } from '$lib/state/index.ts';

/** A selection with one card taken in, or taken out when it was in. */
export function toggled(selected: readonly string[], cardId: string): string[] {
  return selected.includes(cardId) ? selected.filter((id) => id !== cardId) : [...selected, cardId];
}

/**
 * A selection grown by a range: every card from `anchor` to `cardId` in the
 * order the view shows them (a board column by column, top to bottom). Without
 * an anchor in the view the card alone is added.
 */
export function withRange(
  order: readonly string[],
  selected: readonly string[],
  anchor: string | undefined,
  cardId: string,
): string[] {
  const from = anchor === undefined ? -1 : order.indexOf(anchor);
  const to = order.indexOf(cardId);
  if (to === -1) return [...selected];
  const range = from === -1 ? [cardId] : order.slice(Math.min(from, to), Math.max(from, to) + 1);
  return [...selected, ...range.filter((id) => !selected.includes(id))];
}

export class Selection {
  #selected = $state.raw<readonly string[]>([]);
  /** A plain press selects instead of opening. */
  selecting = $state(false);
  /** The last press would have taken in more cards than may be selected. */
  full = $state(false);
  /** The card a range is measured from: the one pressed last. */
  #anchor: string | undefined;
  readonly #shown: () => readonly string[];

  /** `shown`: the cards of the view in the order it shows them. */
  constructor(shown: () => readonly string[]) {
    this.#shown = shown;
  }

  /** The selected cards the view shows. One that left the view (deleted, filtered out) is not counted. */
  readonly ids = $derived.by(() => {
    const shown = this.#shown();
    return this.#selected.filter((id) => shown.includes(id));
  });

  has(cardId: string): boolean {
    return this.ids.includes(cardId);
  }

  #set(next: readonly string[]): void {
    this.full = next.length > CARD_BATCH_LIMIT;
    this.#selected = this.full ? next.slice(0, CARD_BATCH_LIMIT) : next;
    if (next.length === 0) this.selecting = false;
  }

  /** Takes a card in, or out when it was in. */
  toggle(cardId: string): void {
    this.#set(toggled(this.ids, cardId));
    this.#anchor = cardId;
  }

  /** Takes in every card from the one pressed last to this one. */
  extendTo(cardId: string): void {
    this.#set(withRange(this.#shown(), this.ids, this.#anchor, cardId));
    this.#anchor = cardId;
  }

  /** "Select" in the menu of a card: from then on a plain press selects. */
  begin(cardId: string): void {
    this.selecting = true;
    this.toggle(cardId);
  }

  clear(): void {
    this.#set([]);
    this.#anchor = undefined;
  }

  /** Escape in the view lets the selection go, unless something else (a drag, a menu) took the key. */
  readonly attachEscape = (root: HTMLElement): (() => void) => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== 'Escape' || event.defaultPrevented || this.ids.length === 0) return;
      this.clear();
    };
    root.addEventListener('keydown', onKeyDown);
    return () => {
      root.removeEventListener('keydown', onKeyDown);
    };
  };
}
