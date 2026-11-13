/** Board writes restore only their own optimistic values when refused. */
import type { Board, BoardPatch } from '../logic/wire.ts';

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
        () => host.workspace.backend.patchBoard(host.boardId, written),
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
