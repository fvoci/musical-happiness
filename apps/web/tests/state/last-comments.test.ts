/**
 * The time of a card's last comment: what the card list says, kept current
 * by the board as comments come and go, as the contract says a client does.
 */
import { describe, expect, it, vi } from 'vitest';

import { openFirstBoard, settle, text } from './support.ts';

async function onBeta() {
  const made = await openFirstBoard();
  const beta = made.board.realCards.find((card) => card.title === 'Beta');
  if (!beta) throw new Error('no card');
  return { ...made, betaId: beta.id };
}

describe('the memory backend', () => {
  it('says when the newest comment directly on a card was posted, and not in a change message', async () => {
    const { backend, board, betaId } = await onBeta();
    const listed = () => backend.cards(board.boardId);
    const seeded = (await listed()).find((card) => card.id === betaId)?.lastCommentAt ?? -1;
    // The seed's comments, if any, are older than any posted now.
    expect(seeded).toBeGreaterThanOrEqual(0);

    const first = await backend.createComment(board.boardId, betaId, text('one'));
    const second = await backend.createComment(board.boardId, betaId, text('two'));
    expect((await listed()).find((card) => card.id === betaId)?.lastCommentAt).toBe(
      second.createAt,
    );
    // A changed comment does not move it; a deleted one does not count.
    await backend.patchComment(board.boardId, first.id, {
      body: text('one, changed'),
      readAt: first.updateAt,
    });
    expect((await backend.card(board.boardId, betaId)).lastCommentAt).toBe(second.createAt);
    await backend.deleteBlock(board.boardId, second.id);
    expect((await backend.card(board.boardId, betaId)).lastCommentAt).toBe(first.createAt);

    const heard: unknown[] = [];
    const stop = backend.watchBoard(board.boardId, (change) => {
      if (change.kind === 'blocks') heard.push(...change.blocks);
    });
    await backend.patchCard(board.boardId, betaId, { title: 'Beta again' });
    await settle();
    stop();
    expect(heard).toHaveLength(1);
    expect(heard[0]).not.toHaveProperty('lastCommentAt');
  });
});

describe('an open board', () => {
  it('moves a card’s time on when somebody comments, and keeps it through a change of the card', async () => {
    const { board, other, betaId } = await onBeta();
    const posted = await other.createComment(board.boardId, betaId, text('hello'));
    await settle();
    expect(board.card(betaId)?.lastCommentAt).toBe(posted.createAt);

    // The card comes in a change message, without the time: it keeps it.
    await other.patchCard(board.boardId, betaId, { title: 'Beta, renamed' });
    await settle();
    expect(board.card(betaId)).toMatchObject({
      title: 'Beta, renamed',
      lastCommentAt: posted.createAt,
    });
  });

  it('reads the card again when a comment it heard of is deleted', async () => {
    const { board, backend, other, betaId } = await onBeta();
    const older = await other.createComment(board.boardId, betaId, text('older'));
    const newer = await other.createComment(board.boardId, betaId, text('newer'));
    await settle();
    expect(board.card(betaId)?.lastCommentAt).toBe(newer.createAt);

    // The reader, the board's admin, deletes somebody else's comment.
    await backend.deleteBlock(board.boardId, newer.id);
    await settle();
    await vi.waitFor(() => {
      expect(board.card(betaId)?.lastCommentAt).toBe(older.createAt);
    });
    // Brought back, it counts again.
    await backend.restoreBlock(board.boardId, newer.id);
    await settle();
    expect(board.card(betaId)?.lastCommentAt).toBe(newer.createAt);
  });

  it('reads the board again when a comment of no card it knows is deleted', async () => {
    const { board, backend, world, betaId } = await onBeta();
    // Posted while nobody watched: the board never heard of it.
    const unseen = world.newId();
    const at = world.now();
    world.blocks.set(unseen, {
      id: unseen,
      boardId: board.boardId,
      parentId: betaId,
      createdBy: backend.userId,
      modifiedBy: backend.userId,
      schema: 1,
      type: 'comment',
      title: '',
      fields: {},
      createAt: at,
      updateAt: at,
      deleteAt: 0,
      body: text('unseen'),
    });
    const before = board.card(betaId)?.lastCommentAt;
    const cards = vi.spyOn(backend, 'cards');
    await backend.deleteBlock(board.boardId, unseen);
    await settle();
    await vi.waitFor(() => {
      expect(cards).toHaveBeenCalled();
    });
    expect(board.card(betaId)?.lastCommentAt).toBe(before);
  });
});
