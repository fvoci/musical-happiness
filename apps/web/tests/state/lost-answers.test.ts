/**
 * A write whose answer was lost (status 0) but which the server stored is
 * a change like any other: it stays on the screen, and undo takes it back.
 * One that was not stored is taken back as a refused one.
 */
import { describe, expect, it, vi } from 'vitest';

import { BackendError, type Backend } from '$lib/state/index.ts';

import { openFirstBoard } from './support.ts';

type Lost = 'patchBoard' | 'patchCard' | 'patchView';

/** The next call of `method` reaches the backend (when `stored`), and its answer is lost. */
function loseAnswer(backend: Backend, method: Lost, stored: boolean): void {
  const real = backend[method].bind(backend) as (...args: unknown[]) => Promise<unknown>;
  vi.spyOn(backend, method).mockImplementationOnce(async (...args: unknown[]) => {
    if (stored) await real(...args);
    throw new BackendError(0, { detail: 'no answer' });
  });
}

async function cases() {
  const opened = await openFirstBoard();
  const { board, backend } = opened;
  const card = board.realCards[0];
  const view = board.views[0];
  if (!card || !view) throw new Error('the seed board has no card or view');
  const kinds = {
    patchBoard: {
      change: () => board.changeBoard((current) => ({ ...current, title: 'Changed' })),
      shown: () => board.board?.title,
      stored: async () => (await backend.board(board.boardId)).title,
      before: board.board?.title,
    },
    patchCard: {
      change: () => board.setCardTitle(card.id, 'Changed'),
      shown: () => board.card(card.id)?.title,
      stored: async () => (await backend.card(board.boardId, card.id)).title,
      before: card.title,
    },
    patchView: {
      change: () => board.changeView(view.id, (current) => ({ ...current, title: 'Changed' })),
      shown: () => board.view(view.id)?.title,
      stored: async () =>
        (await backend.views(board.boardId)).find((one) => one.id === view.id)?.title,
      before: view.title,
    },
  };
  return { ...opened, kinds };
}

describe.each(['patchBoard', 'patchCard', 'patchView'] as const)('%s', (method) => {
  it('stored, though its answer was lost, stays and is undone', async () => {
    const { backend, workspace, kinds } = await cases();
    const kind = kinds[method];
    const report = vi.spyOn(workspace, 'report');
    try {
      loseAnswer(backend, method, true);
      await kind.change();
      expect([kind.shown(), await kind.stored()]).toEqual(['Changed', 'Changed']);
      expect(report).not.toHaveBeenCalled();
      await workspace.undoLast();
      expect([kind.shown(), await kind.stored()]).toEqual([kind.before, kind.before]);
    } finally {
      workspace.close();
    }
  });

  it('not stored, its answer lost, is taken back as refused', async () => {
    const { backend, workspace, kinds } = await cases();
    const kind = kinds[method];
    try {
      loseAnswer(backend, method, false);
      await kind.change();
      expect([kind.shown(), await kind.stored()]).toEqual([kind.before, kind.before]);
      expect(workspace.undo.canUndo).toBe(false);
    } finally {
      workspace.close();
    }
  });
});
