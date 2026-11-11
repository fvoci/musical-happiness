/**
 * An open card: its body is written whole with the version it was read at,
 * and a refused write never overwrites what somebody else wrote.
 */
import { describe, expect, it, vi } from 'vitest';

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

  it('waits for a write that is under way when asked to store with nothing new', async () => {
    const { card, backend, board, cardId } = await openAlpha();
    backend.latency = 5;
    void card.saveBody(text('on its way'));
    expect(card.saving).toBe(true);
    // The caller that closes the card asks once more and leaves when this answers.
    await card.saveBody();
    expect(card.saving).toBe(false);
    expect(card.dirty).toBe(false);
    expect((await backend.card(board.boardId, cardId)).body).toEqual(text('on its way'));
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

  it('keeps sending the body when something else changes again while it is sent again', async () => {
    const { card, other, backend, board, cardId } = await openAlpha();
    card.editBody(text('mine'));
    await other.patchCard(board.boardId, cardId, { title: 'Renamed once' });
    // Right after the store reads the card again, the title changes once more.
    const read = backend.card.bind(backend);
    let reads = 0;
    vi.spyOn(backend, 'card').mockImplementation(async (...args) => {
      const found = await read(...args);
      reads += 1;
      if (reads === 1) await other.patchCard(board.boardId, cardId, { title: 'Renamed twice' });
      return found;
    });
    await card.saveBody();
    expect(reads).toBe(2);
    expect(card.conflict).toBeUndefined();
    const stored = await read(board.boardId, cardId);
    expect(stored).toMatchObject({ body: text('mine'), title: 'Renamed twice' });
  });

  it('does not send a body the server would refuse: a mention that names nobody', async () => {
    const { card, backend, workspace } = await openAlpha();
    const write = vi.spyOn(backend, 'writeCardBody');
    const anonymous = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'mention', attrs: { userId: '' } }] }],
    };
    await card.saveBody(anonymous);
    expect(write).not.toHaveBeenCalled();
    expect(card.dirty).toBe(true);
    expect(workspace.problems.at(-1)).toMatchObject({
      action: 'save',
      error: { status: 400, errors: [{ pointer: '/body', reason: 'invalid' }] },
    });
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

  it('changes the text of a comment from the version it was begun on, and refuses to write over a newer one', async () => {
    const { card, other, board } = await openAlpha();
    expect(await card.addComment(text('mine'))).toBe(true);
    const mine = card.comments.at(-1);
    if (!mine) throw new Error('no comment');
    expect(await card.editComment(mine.id, text('mine, changed'), mine.updateAt)).toBe('saved');
    expect(card.comment(mine.id)?.body).toEqual(text('mine, changed'));

    // The reader begins on this version; somebody else changes it, and that is heard.
    const begun = card.comment(mine.id)?.updateAt ?? 0;
    await other.patchComment(board.boardId, mine.id, { body: text('theirs'), readAt: begun });
    await settle();
    expect(card.comment(mine.id)?.body).toEqual(text('theirs'));
    // Saved on the version begun from: refused, theirs stays.
    const refresh = vi.spyOn(card, 'refresh');
    expect(await card.editComment(mine.id, text('again'), begun)).toBe('conflict');
    expect(refresh).toHaveBeenCalledOnce();
    expect(card.comment(mine.id)?.body).toEqual(text('theirs'));
    // On the version shown now it goes.
    const now = card.comment(mine.id)?.updateAt ?? 0;
    expect(await card.editComment(mine.id, text('again'), now)).toBe('saved');
    expect(card.comment(mine.id)?.body).toEqual(text('again'));
  });

  it('tells a comment change that failed for another reason', async () => {
    const { card, backend, workspace } = await openAlpha();
    const first = card.comments[0];
    if (!first) throw new Error('no comment');
    backend.failNext('patchComment', new BackendError(500));
    expect(await card.editComment(first.id, text('x'), first.updateAt)).toBe('failed');
    expect(workspace.problems.at(-1)).toMatchObject({ action: 'save', error: { status: 500 } });
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

  it('uploads an image for the body out of the list, and keeps the body while it goes up', async () => {
    const { card, other, backend, board, cardId } = await openAlpha();
    backend.latency = 5;
    const uploading = card.uploadImage(new File(['png'], 'a.png', { type: 'image/png' }));
    expect(card.uploads).toHaveLength(0);
    // Somebody else writes the body meanwhile: it does not replace the one shown.
    const theirs = await other.card(board.boardId, cardId);
    await other.writeCardBody(board.boardId, cardId, {
      body: text('theirs'),
      readAt: theirs.updateAt,
    });
    await settle();
    expect(card.body).toEqual(text('first body'));
    expect(await uploading).toMatch(/./);
  });

  it('gives up an image upload when the card is closed, telling nothing', async () => {
    const { card, backend, workspace } = await openAlpha();
    backend.latency = 5;
    const told = workspace.problems.length;
    const uploading = card.uploadImage(new File(['png'], 'a.png', { type: 'image/png' }));
    card.close();
    await expect(uploading).rejects.toThrow();
    expect(workspace.problems).toHaveLength(told);
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

describe('deleted and brought back', () => {
  it('an open card deleted by somebody else is missing, and is there again once restored', async () => {
    const { card, other, board, cardId } = await openAlpha();
    await other.deleteBlock(board.boardId, cardId);
    await settle();
    expect(card.missing).toBe(true);
    await other.restoreBlock(board.boardId, cardId);
    await settle();
    await vi.waitFor(() => {
      expect(card.missing).toBe(false);
    });
    expect(card.summary?.title).toBe('Alpha');
  });

  it('undo of a delete counts once the card is restored, even if reading it fails', async () => {
    const { board, workspace, backend, cardId } = await openAlpha();
    await board.deleteCards([cardId]);
    vi.spyOn(backend, 'card').mockRejectedValueOnce(new BackendError(500, { detail: 'down' }));
    await workspace.undoLast();
    expect(workspace.problems).toHaveLength(1);
    // Not undone again: the next undo would restore it twice.
    expect(workspace.undo.canUndo).toBe(false);
    await settle();
    expect(board.card(cardId)?.title).toBe('Alpha');
  });

  it('undo of a card made goes on when somebody else deleted it first', async () => {
    const { board, workspace, other } = await openAlpha();
    const made = await board.createCard({ title: 'Brief', fields: {} } as never);
    if (!made) throw new Error('not made');
    await other.deleteBlock(board.boardId, made.id);
    await settle();
    await workspace.undoLast();
    expect(workspace.problems).toHaveLength(0);
    expect(board.card(made.id)).toBeUndefined();
  });
});

describe('who deletes a comment', () => {
  it('an admin anyone’s, an editor their own, a commenter none', async () => {
    const { board, workspace } = await openAlpha();
    const me = workspace.me?.id ?? '';
    const as = (
      role: Partial<Record<'schemeAdmin' | 'schemeEditor' | 'schemeCommenter', boolean>>,
    ) => {
      board.members = board.members.map((member) =>
        member.userId === me
          ? { ...member, schemeAdmin: false, schemeEditor: false, schemeCommenter: false, ...role }
          : member,
      );
    };
    expect(board.role).toBe('admin');
    expect(board.canDeleteComment('someone')).toBe(true);
    as({ schemeEditor: true });
    expect([board.canDeleteComment(me), board.canDeleteComment('someone')]).toEqual([true, false]);
    as({ schemeCommenter: true });
    expect(board.canDeleteComment(me)).toBe(false);
  });
});
