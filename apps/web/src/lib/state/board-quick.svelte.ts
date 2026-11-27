/**
 * The quick filters a reader has turned on, board by board
 * (lib/logic/quick-filter.ts). They are the reader's alone: never written to
 * the view or sent to the server.
 *
 * A signed-in reader's are kept in this browser's storage under the board,
 * as the names of the chips and nothing else, and forgotten when they sign
 * out (`forgetUnscoped` in src/app/last-workspace.ts). A reader without an
 * account (a public link), or a browser without storage, keeps them for the
 * visit alone.
 *
 * Besides, the cards a reader made on a visit to a board are let through
 * every chip until the visit ends (`madeOnVisit`), so that a card does not
 * vanish as it is made because it is nobody's yet or has no due date. It is
 * not given values to meet them, as a view's filter gives a new card.
 */
import { isQuickFilterId, type QuickFilterId } from '../logic/quick-filter.ts';
import type { Card } from '../logic/wire.ts';

/** The beginning of the key of a board's chips in `localStorage`. */
export const QUICK_FILTERS_KEY = 'fvoci.quick-filters.';

/** Where chips are kept: in the browser's storage, or for the visit alone. */
export type QuickFilterKeeping = 'browser' | 'visit';

export class QuickFilterChoices {
  /**
   * The chips of each board turned on or off on this page. A board that is
   * not here has those kept in storage, read every time they are asked for:
   * reading them is no change a derivation could not make.
   */
  #chosen = $state.raw<Readonly<Record<string, readonly QuickFilterId[]>>>({});
  readonly #storage: () => Storage | undefined;

  constructor(storage: () => Storage | undefined = () => globalThis.localStorage) {
    this.#storage = storage;
  }

  /** The chips turned on for a board, in the order they were. */
  chosen(boardId: string): readonly QuickFilterId[] {
    return this.#chosen[boardId] ?? this.#read(boardId);
  }

  /** Turns a chip on, or off when it is on. */
  toggle(boardId: string, id: QuickFilterId, keeping: QuickFilterKeeping): void {
    const chosen = this.chosen(boardId);
    this.#set(
      boardId,
      chosen.includes(id) ? chosen.filter((other) => other !== id) : [...chosen, id],
      keeping,
    );
  }

  /** Turns off every chip of a board. */
  clear(boardId: string, keeping: QuickFilterKeeping): void {
    this.#set(boardId, [], keeping);
  }

  #set(boardId: string, chosen: readonly QuickFilterId[], keeping: QuickFilterKeeping): void {
    this.#chosen = { ...this.#chosen, [boardId]: chosen };
    if (keeping === 'browser') this.#keep(boardId, chosen);
  }

  #read(boardId: string): QuickFilterId[] {
    try {
      const kept: unknown = JSON.parse(
        this.#storage()?.getItem(QUICK_FILTERS_KEY + boardId) ?? '[]',
      );
      if (!Array.isArray(kept)) return [];
      const known = kept.filter(isQuickFilterId);
      return known.filter((id, at) => known.indexOf(id) === at);
    } catch {
      return [];
    }
  }

  #keep(boardId: string, chosen: readonly QuickFilterId[]): void {
    try {
      const storage = this.#storage();
      if (chosen.length === 0) storage?.removeItem(QUICK_FILTERS_KEY + boardId);
      else storage?.setItem(QUICK_FILTERS_KEY + boardId, JSON.stringify(chosen));
    } catch {
      // Without storage the chips stay on for the visit.
    }
  }
}

/** The chips of this page, one for every board it opens. */
export const quickFilters = new QuickFilterChoices();

/** What `madeOnVisit` reads of a board store: the cards, and which visit of it this is. */
export interface VisitedBoard {
  readonly status?: string;
  /** Counts the visits to the board; another number is another visit. */
  readonly visit?: number;
  readonly realCards: readonly Card[];
}

/** The cards a board had when a visit to it started. */
const atStart = new WeakMap<VisitedBoard, { visit: number | undefined; ids: Set<string> }>();

/**
 * Which cards `meId` made on this visit to the board: those that were not
 * among the cards the board had when the visit started, and that the reader
 * made. The start is the first time this is asked once the board is loaded,
 * whether chips are on or not, so it is to be asked every time the cards are
 * read; before then no card was made on the visit. Nobody makes a card on a
 * visit they are not signed in to.
 */
export function madeOnVisit(
  board: VisitedBoard,
  meId: string | undefined,
): (card: Card) => boolean {
  const start = startOf(board);
  if (meId === undefined || start === undefined) return () => false;
  return (card) => card.createdBy === meId && !start.has(card.id);
}

/** The cards of a board at the start of the visit it is on, once it is loaded. */
function startOf(board: VisitedBoard): Set<string> | undefined {
  if (board.status !== undefined && board.status !== 'ready') return undefined;
  const known = atStart.get(board);
  if (known && known.visit === board.visit) return known.ids;
  const ids = new Set(board.realCards.map((card) => card.id));
  atStart.set(board, { visit: board.visit, ids });
  return ids;
}
