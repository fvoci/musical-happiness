/**
 * The writes of a board, its cards and its views. A refused board write
 * restores only its own optimistic values.
 *
 * A write whose answer was lost (no answer came: status 0) may have been
 * stored all the same. Before it is taken for refused (its step leaves the
 * history, the screen takes it back), the thing is read again: when what is
 * stored already says everything the write said, the write was stored, and
 * that reading is its answer. A refusal the server answered is a refusal.
 * What is stored may say the same because somebody else wrote the same
 * meanwhile, and a value a later change replaced cannot be told apart: the
 * write is then taken for stored, and its undo writes back a value the
 * reader did make.
 */
import type { BlockPatch, Board, BoardPatch, BoardView } from '../logic/wire.ts';

import { isBackendError, type CardPatch, type CardSummary } from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import { InverseValues, sameValue } from './inverse-values.ts';
import { Inverses } from './inverses.ts';
import { applyBoardPatch } from './patches.ts';
import { PendingWrites } from './write.ts';

export class BoardWrites {
  readonly #host: BoardStore;
  readonly #pending = new PendingWrites<Board>();
  readonly #inverses = new Inverses<BoardPatch>('updatedProperties', 'deletedProperties');
  readonly #values = new InverseValues('updatedProperties', 'deletedProperties');

  constructor(host: BoardStore) {
    this.#host = host;
  }

  receive(board: Board): boolean {
    return this.#pending.receive(board);
  }

  keep(reverse: BoardPatch): void {
    this.#inverses.keep(this.#host.boardId, reverse);
  }

  async write(patch: BoardPatch, reverse: BoardPatch): Promise<void> {
    const host = this.#host;
    if (!host.board) return;
    // Inverses can be corrected while requests wait. A request already sent has fixed values.
    const written = structuredClone(patch);
    host.board = applyBoardPatch(host.board, written);
    this.#pending.begin(host.boardId);
    let answer: Board | undefined;
    let restore: BoardPatch = {};
    try {
      answer = await this.#inverses.write(
        host.boardId,
        written,
        reverse,
        (own) => patchBoardOrStored(host, written, own),
        (refused) => {
          restore = refused;
        },
      );
    } finally {
      // Restore even if a held server version is older than a board put on screen directly.
      if (!answer) host.board = this.#restore(host.board, written, restore);
      const latest = this.#pending.end(host.boardId, answer);
      if (latest && latest.updateAt >= host.board.updateAt) host.board = latest;
    }
  }

  #restore(shown: Board, written: BoardPatch, restore: BoardPatch): Board {
    // Changes outside this writer may have replaced a field too: keep those values.
    const current = this.#values.read({
      ...shown,
      updatedProperties: shown.properties,
      updatedCardProperties: shown.cardProperties,
    });
    const failed = this.#values.read(written as Record<string, unknown>);
    const own: Record<string, unknown> = {};
    for (const [key, value] of this.#values.read(restore as Record<string, unknown>)) {
      if (sameValue(current.get(key), failed.get(key))) this.#values.put(own, key, value);
    }
    return applyBoardPatch(shown, own);
  }
}

/** A thing as the values a patch of it names (`InverseValues`). */
type AsPatch<T> = (thing: T) => Record<string, unknown>;

/**
 * `send`'s answer, or, when it was lost, what `read` finds if it holds the
 * write: every value of `patch` that is still the write's on the screen
 * (`own`: no later change replaced it, whatever value that change shows)
 * is stored. A value a later change replaced is not asked for: that change
 * may have been stored over it before the reading.
 */
async function answeredOrStored<T>(
  patch: object,
  values: InverseValues,
  as: AsPatch<T>,
  ways: {
    send: () => Promise<T>;
    read: () => Promise<T | undefined>;
    own: (key: string) => boolean;
  },
): Promise<T> {
  try {
    return await ways.send();
  } catch (error) {
    if (!isBackendError(error) || error.status !== 0) throw error;
    const stored = await ways.read().catch(() => undefined);
    if (stored === undefined) throw error;
    const storedValues = values.read(as(stored));
    for (const [key, value] of values.read(patch as Record<string, unknown>)) {
      if (ways.own(key) && !sameValue(storedValues.get(key), value)) {
        throw error;
      }
    }
    return stored;
  }
}

const boardValues = new InverseValues('updatedProperties', 'deletedProperties');
const viewValues = new InverseValues('updatedFields', 'deletedFields');

export function patchBoardOrStored(
  store: BoardStore,
  patch: BoardPatch,
  own: (key: string) => boolean,
): Promise<Board> {
  const { backend } = store.workspace;
  return answeredOrStored(
    patch,
    boardValues,
    (board: Board) => ({
      ...board,
      updatedProperties: board.properties,
      updatedCardProperties: board.cardProperties,
    }),
    {
      send: () => backend.patchBoard(store.boardId, patch),
      read: () => backend.board(store.boardId),
      own,
    },
  );
}

export function patchCardOrStored(
  store: BoardStore,
  cardId: string,
  patch: CardPatch,
  own: (key: string) => boolean,
): Promise<CardSummary> {
  const { backend } = store.workspace;
  return answeredOrStored(
    patch,
    boardValues,
    (card: CardSummary) => ({
      title: card.title,
      icon: card.fields.icon,
      updatedProperties: card.fields.properties,
    }),
    {
      send: () => backend.patchCard(store.boardId, cardId, patch),
      read: async () => {
        const { body: _body, ...card } = await backend.card(store.boardId, cardId);
        return card;
      },
      own,
    },
  );
}

export function patchViewOrStored(
  store: BoardStore,
  viewId: string,
  patch: BlockPatch,
  own: (key: string) => boolean,
): Promise<BoardView> {
  const { backend } = store.workspace;
  return answeredOrStored(
    patch,
    viewValues,
    (view: BoardView) => ({ title: view.title, updatedFields: view.fields }),
    {
      send: () => backend.patchView(store.boardId, viewId, patch),
      read: async () => (await backend.views(store.boardId)).find((view) => view.id === viewId),
      own,
    },
  );
}
