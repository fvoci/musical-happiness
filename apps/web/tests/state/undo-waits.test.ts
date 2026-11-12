/**
 * Undo and redo on a backend that answers 400ms late, through the board
 * store: Ctrl+Z right after a change takes back that change; a refused
 * change leaves the undo that waited for it nothing to do; a change made
 * while an undo waits drops it; a gesture of several writes is one step
 * from its start.
 */
import { describe, expect, it } from 'vitest';

import { BackendError } from '$lib/state/index.ts';

import { openFirstBoard, STATUS } from './support.ts';

const LATE = 400;

async function slowBoard() {
  const made = await openFirstBoard();
  const { board, backend } = made;
  const alpha = board.realCards.find((card) => card.title === 'Alpha')?.id ?? '';
  const title = (): string | undefined => board.card(alpha)?.title;
  const stored = async (): Promise<string> => (await backend.card(board.boardId, alpha)).title;
  await board.setCardTitle(alpha, 'One');
  backend.latency = LATE;
  /** The writes of cards asked of the backend from here on. */
  const sent: unknown[] = [];
  const patchCard = backend.patchCard.bind(backend);
  backend.patchCard = (boardId, cardId, patch) => {
    sent.push(patch);
    return patchCard(boardId, cardId, patch);
  };
  return { ...made, alpha, title, stored, sent };
}

describe('undo asked for while a change is being written', () => {
  it('takes back that change, not the one before', async () => {
    const { board, workspace, alpha, title, stored } = await slowBoard();
    const changing = board.setCardTitle(alpha, 'Two');
    const undoing = workspace.undoLast();
    expect(workspace.undoBusy).toBe(true);
    await Promise.all([changing, undoing]);
    expect(title()).toBe('One');
    expect(await stored()).toBe('One');
    expect(workspace.undoBusy).toBe(false);
    // "One" is still in force and the next to undo; "Two" the next to redo.
    expect(workspace.canUndo).toBe(true);
    expect(workspace.canRedo).toBe(true);
  });

  it('is dropped when the change is refused, and the change before stays', async () => {
    const { board, backend, workspace, alpha, title, stored } = await slowBoard();
    backend.failNext('patchCard', new BackendError(403));
    const changing = board.setCardTitle(alpha, 'Two');
    const undoing = workspace.undoLast();
    await Promise.all([changing, undoing]);
    expect(title()).toBe('One');
    expect(await stored()).toBe('One');
    expect(workspace.problems.map((problem) => problem.action)).toEqual(['save']);
    expect(workspace.canUndo).toBe(true);
    expect(workspace.canRedo).toBe(false);
  });

  it('is dropped by a change made while it waits, which is made at once', async () => {
    const { board, workspace, alpha, title, stored } = await slowBoard();
    const renaming = board.setCardTitle(alpha, 'Two');
    const undoing = workspace.undoLast();
    const changing = board.setCardProperty(alpha, STATUS, 'done');
    expect(board.card(alpha)?.fields.properties[STATUS]).toBe('done');
    await Promise.all([renaming, undoing, changing]);
    expect(title()).toBe('Two');
    expect(await stored()).toBe('Two');
    expect(board.card(alpha)?.fields.properties[STATUS]).toBe('done');
    // The steps go on from what is in force: the status, then "Two", then "One".
    await workspace.undoLast();
    expect(board.card(alpha)?.fields.properties[STATUS]).toBe('todo');
    await workspace.undoLast();
    expect(title()).toBe('One');
  });
});

describe('redo asked for while an undo is being written', () => {
  it('waits for it, and does the change again', async () => {
    const { workspace, title, stored, sent } = await slowBoard();
    const undoing = workspace.undoLast();
    const redoing = workspace.redoLast();
    await Promise.all([undoing, redoing]);
    expect(sent).toEqual([{ title: 'Alpha' }, { title: 'One' }]);
    expect(title()).toBe('One');
    expect(await stored()).toBe('One');
  });

  it('is dropped when the undo is refused', async () => {
    const { backend, workspace, title, stored, sent } = await slowBoard();
    backend.failNext('patchCard', new BackendError(403));
    const undoing = workspace.undoLast();
    const redoing = workspace.redoLast();
    await Promise.all([undoing, redoing]);
    // The undo was sent, and the redo was not.
    expect(sent).toEqual([{ title: 'Alpha' }]);
    expect(title()).toBe('One');
    expect(await stored()).toBe('One');
    expect(workspace.problems.map((problem) => problem.action)).toEqual(['undo']);
    expect(workspace.canUndo).toBe(true);
    expect(workspace.canRedo).toBe(false);
  });
});

describe('a gesture of several writes', () => {
  it('is taken back whole by an undo asked for before its writes are done', async () => {
    const { board, backend, workspace, title } = await slowBoard();
    const ids = board.realCards.slice(0, 2).map((card) => card.id);
    const deleting = board.deleteCards(ids);
    const undoing = workspace.undoLast();
    await Promise.all([deleting, undoing]);
    expect(board.realCards.filter((card) => ids.includes(card.id))).toHaveLength(ids.length);
    const kept = await backend.cards(board.boardId);
    expect(kept.filter((card) => ids.includes(card.id))).toHaveLength(ids.length);
    // The title changed before the gesture is still in force.
    expect(title()).toBe('One');
  });
});
