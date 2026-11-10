/**
 * An open card: its body is written whole with the version it was read at,
 * and a refused write never overwrites what somebody else wrote.
 */
import { describe, expect, it } from 'vitest';

import { BackendError } from '$lib/state/index.ts';

import { openFirstBoard, settle, STATUS, text } from './support.ts';

async function openAlpha() {
  const made = await openFirstBoard();
  const alpha = made.board.realCards.find((card) => card.title === 'Alpha');
  if (!alpha) throw new Error('no card');
  const card = await made.board.showCard(alpha.id);
  return { ...made, card, cardId: alpha.id };
}

describe('an open card', () => {
  it('reads the body, the comments and the attachments, oldest first', async () => {
    const { card } = await openAlpha();
    expect(card.status).toBe('ready');
    expect(card.body).toEqual(text('first body'));
    expect(card.bodyRevision).toBe(1);
    expect(card.comments).toHaveLength(2);
    expect(card.comments[0]?.createAt).toBeLessThan(card.comments[1]?.createAt ?? 0);
    expect(card.attachments.map((attachment) => attachment.name)).toEqual(['kick-off-notes.txt']);
    expect(card.summary?.title).toBe('Alpha');
  });

  it('is missing when the card is not there', async () => {
    const { board } = await openFirstBoard();
    const card = await board.showCard('nope');
    expect(card.missing).toBe(true);
  });

  it('stores the body and does not make the editor take it again', async () => {
    const { card, backend, board, cardId } = await openAlpha();
    await card.saveBody(text('edited'));
    expect(card.saving).toBe(false);
    expect(card.dirty).toBe(false);
    expect(card.bodyRevision).toBe(1);
    expect((await backend.card(board.boardId, cardId)).body).toEqual(text('edited'));
  });

  it('stores the newest of several bodies given while one was being written', async () => {
    const { card, backend, board, cardId } = await openAlpha();
    backend.latency = 5;
    const first = card.saveBody(text('one'));
    void card.saveBody(text('two'));
    const last = card.saveBody(text('three'));
    expect(card.saving).toBe(true);
    await Promise.all([first, last]);
    expect(card.saving).toBe(false);
    expect((await backend.card(board.boardId, cardId)).body).toEqual(text('three'));
  });

  it('writes on the newest version it knows after the reader changed a property', async () => {
    const { card, board, backend, workspace, cardId } = await openAlpha();
    await board.setCardProperty(cardId, STATUS, 'done');
    await card.saveBody(text('after a property change'));
    expect(card.conflict).toBeUndefined();
    expect(workspace.problems).toHaveLength(0);
    expect((await backend.card(board.boardId, cardId)).body).toEqual(
      text('after a property change'),
    );
  });

  it('sends the body again by itself when only something else about the card changed', async () => {
    const { card, other, backend, board, cardId } = await openAlpha();
    card.editBody(text('mine'));
    await other.patchCard(board.boardId, cardId, { title: 'Alpha, renamed' });
    await settle();
    await card.saveBody();
    expect(card.conflict).toBeUndefined();
    const stored = await backend.card(board.boardId, cardId);
    expect(stored.body).toEqual(text('mine'));
    expect(stored.title).toBe('Alpha, renamed');
  });

  it('holds both sides when somebody else wrote the body, and overwrites nothing', async () => {
    const { card, other, backend, board, workspace, cardId } = await openAlpha();
    const theirs = await other.card(board.boardId, cardId);
    await other.writeCardBody(board.boardId, cardId, {
      body: text('theirs'),
      readAt: theirs.updateAt,
    });
    await card.saveBody(text('mine'));
    expect(card.conflict?.mine).toEqual(text('mine'));
    expect(card.conflict?.theirs.body).toEqual(text('theirs'));
    expect(card.dirty).toBe(true);
    expect(workspace.problems.at(-1)?.action).toBe('conflict');
    expect((await backend.card(board.boardId, cardId)).body).toEqual(text('theirs'));
  });

  it("settles a conflict the reader's way: theirs is shown, or mine is stored", async () => {
    const first = await openAlpha();
    first.card.editBody(text('mine'));
    const stale = await first.other.card(first.board.boardId, first.cardId);
    await first.other.writeCardBody(first.board.boardId, first.cardId, {
      body: text('theirs'),
      readAt: stale.updateAt,
    });
    await first.card.saveBody();
    const revision = first.card.bodyRevision;
    await first.card.resolveConflict('theirs');
    expect(first.card.body).toEqual(text('theirs'));
    expect(first.card.bodyRevision).toBe(revision + 1);
    expect(first.card.dirty).toBe(false);

    const second = await openAlpha();
    second.card.editBody(text('mine'));
    const before = await second.other.card(second.board.boardId, second.cardId);
    await second.other.writeCardBody(second.board.boardId, second.cardId, {
      body: text('theirs'),
      readAt: before.updateAt,
    });
    await second.card.saveBody();
    await second.card.resolveConflict('mine');
    expect(second.card.conflict).toBeUndefined();
    expect((await second.backend.card(second.board.boardId, second.cardId)).body).toEqual(
      text('mine'),
    );
  });

  it("takes somebody else's body while the reader has nothing unsaved", async () => {
    const { card, other, board, cardId } = await openAlpha();
    const read = await other.card(board.boardId, cardId);
    await other.writeCardBody(board.boardId, cardId, { body: text('news'), readAt: read.updateAt });
    await settle();
    await settle();
    expect(card.body).toEqual(text('news'));
    expect(card.bodyRevision).toBe(2);
  });

  it('keeps what the reader wrote when a write fails for another reason', async () => {
    const { card, backend, workspace } = await openAlpha();
    backend.failNext('writeCardBody', new BackendError(500));
    await card.saveBody(text('kept'));
    expect(card.dirty).toBe(true);
    expect(workspace.problems.at(-1)).toMatchObject({ action: 'save', error: { status: 500 } });
    await card.saveBody(text('kept'));
    expect(card.dirty).toBe(false);
  });
});

describe('comments and attachments', () => {
  it("adds a comment, sees somebody else's, and brings a deleted one back with undo", async () => {
    const { card, other, board, workspace, cardId } = await openAlpha();
    expect(await card.addComment(text('mine'))).toBe(true);
    await other.createComment(board.boardId, cardId, text('theirs'));
    await settle();
    expect(card.comments).toHaveLength(4);
    const mine = card.comments.find((comment) => JSON.stringify(comment.body).includes('mine'));
    await card.deleteComment(mine?.id ?? '');
    expect(card.comments).toHaveLength(3);
    await workspace.undoLast();
    expect(card.comments).toHaveLength(4);
  });

  it('uploads a file, shows its progress, and attaches it', async () => {
    const { card, backend } = await openAlpha();
    backend.latency = 2;
    const attaching = card.attach(new File(['hello'], 'hello.txt', { type: 'text/plain' }));
    expect(card.uploads.map((upload) => upload.name)).toEqual(['hello.txt']);
    await attaching;
    expect(card.uploads).toHaveLength(0);
    expect(card.attachments.map((attachment) => attachment.name)).toEqual([
      'kick-off-notes.txt',
      'hello.txt',
    ]);
    expect(card.fileUrl(card.attachments[1]?.fileId ?? '')).toMatch(/^blob:/);
  });

  it('keeps a failed upload in the list until it is dismissed, and drops one that was given up', async () => {
    const { card, backend, workspace } = await openAlpha();
    backend.failNext('upload', new BackendError(413, { limit: 10 }));
    await card.attach(new File(['hello'], 'big.bin'));
    expect(card.uploads[0]).toMatchObject({ name: 'big.bin', failed: true });
    expect(workspace.problems.at(-1)).toMatchObject({ action: 'upload', error: { limit: 10 } });
    card.uploads[0]?.cancel();
    expect(card.uploads).toHaveLength(0);

    backend.latency = 5;
    const attaching = card.attach(new File(['hello'], 'stopped.bin'));
    card.uploads[0]?.cancel();
    await attaching;
    expect(card.uploads).toHaveLength(0);
    expect(card.attachments).toHaveLength(1);
  });

  it('closes when its card is deleted by somebody else', async () => {
    const { card, other, board, cardId } = await openAlpha();
    await other.deleteBlock(board.boardId, cardId);
    await settle();
    expect(card.missing).toBe(true);
  });
});
