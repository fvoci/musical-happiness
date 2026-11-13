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
 * meanwhile; the write is then taken for stored too, which is what the
 * screen shows either way.
 */
import type { BlockPatch, Board, BoardPatch } from '../logic/wire.ts';

import { isBackendError, type Backend, type CardPatch, type CardSummary } from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import { InverseValues, sameValue } from './inverse-values.ts';
import { Inverses } from './inverses.ts';
import { applyBoardPatch, applyCardPatch, applyViewPatch } from './patches.ts';
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
        () => patchBoardOrStored(host.workspace.backend, host.boardId, written),
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

/** `send`'s answer, or, when it was lost, what `read` finds if `says` it holds the write. */
export async function answeredOrStored<T>(
  send: () => Promise<T>,
  read: () => Promise<T | undefined>,
  says: (stored: T) => boolean,
): Promise<T> {
  try {
    return await send();
  } catch (error) {
    if (!isBackendError(error) || error.status !== 0) throw error;
    const stored = await read().catch(() => undefined);
    if (stored !== undefined && says(stored)) return stored;
    throw error;
  }
}

export function patchBoardOrStored(
  backend: Backend,
  boardId: string,
  patch: BoardPatch,
): Promise<Board> {
  return answeredOrStored(
    () => backend.patchBoard(boardId, patch),
    () => backend.board(boardId),
    (stored) => sameValue(applyBoardPatch(stored, patch), stored),
  );
}

export function patchCardOrStored(
  backend: Backend,
  boardId: string,
  cardId: string,
  patch: CardPatch,
): Promise<CardSummary> {
  return answeredOrStored(
    () => backend.patchCard(boardId, cardId, patch),
    async () => {
      const { body: _body, ...card } = await backend.card(boardId, cardId);
      return card;
    },
    (stored) => sameValue(applyCardPatch(stored, patch), stored),
  );
}

export function patchViewOrStored(
  backend: Backend,
  boardId: string,
  viewId: string,
  patch: BlockPatch,
) {
  return answeredOrStored(
    () => backend.patchView(boardId, viewId, patch),
    async () => (await backend.views(boardId)).find((view) => view.id === viewId),
    (stored) => sameValue(applyViewPatch(stored, patch), stored),
  );
}
