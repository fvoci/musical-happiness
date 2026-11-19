/**
 * An open card: its body in its room, its comments and attachments, and who
 * may change and delete a comment.
 */
import { describe, expect, it, vi } from 'vitest';
import * as Y from 'yjs';

import { BackendError } from '$lib/state/index.ts';
import { bodyOf, FIELD, roomsOf } from '$lib/state/memory/rooms.ts';

import { openFirstBoard, settle, text } from './support.ts';

async function openAlpha() {
  const made = await openFirstBoard();
  const alpha = made.board.realCards.find((card) => card.title === 'Alpha');
  if (!alpha) throw new Error('no card');
  const card = await made.board.showCard(alpha.id);
  return { ...made, card, cardId: alpha.id };
}

/**
 * Alpha open with its body in its room, and a way to have the reader taken
 * off the board; each state of the room is put on `statuses` as it comes.
 */
async function inTheRoom(statuses: string[] = []) {
  const made = await openAlpha();
  const { card, board, backend, world } = made;
  const openDoc = backend.openDoc.bind(backend);
  vi.spyOn(backend, 'openDoc').mockImplementation((workspaceId, boardId, cardId, hears) =>
    openDoc(workspaceId, boardId, cardId, {
      ...hears,
      state: (state) => {
        statuses.push(state.status);
        hears.state(state);
      },
    }),
  );
  card.openBody();
  await vi.waitFor(() => {
    expect(card.room?.status).toBe('joined');
  });
  const me = board.workspace.me?.id ?? '';
  const otherId = [...world.users.keys()].find((id) => id !== me) ?? '';
  // A member already keeps their role when added: it is set after.
  await backend.addMember(board.boardId, otherId, 'admin');
  await backend.setMemberRole(board.boardId, otherId, 'admin');
  const takenOff = () => made.other.removeMember(board.boardId, me);
  return { ...made, takenOff };
}

describe('an open card', () => {
  it('reads the body, the comments and the attachments, oldest first', async () => {
    const { card } = await openAlpha();
    expect(card.status).toBe('ready');
    expect(card.body).toEqual(text('first body'));
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

  it("takes somebody else's body while it is not in the body's room", async () => {
    const { card, other, board, cardId } = await openAlpha();
    const read = await other.card(board.boardId, cardId);
    await other.writeCardBody(board.boardId, cardId, { body: text('news'), readAt: read.updateAt });
    await settle();
    await settle();
    expect(card.body).toEqual(text('news'));
  });

  it("opens the body in its room, where the room's document wins over the card's body, and leaves it when closed", async () => {
    const { card, other, board, cardId, world } = await openAlpha();
    card.openBody();
    await vi.waitFor(() => {
      expect(card.room?.status).toBe('joined');
    });
    expect(card.room?.writable).toBe(true);
    expect(roomsOf(world).members(cardId)).toBe(1);
    // A change of the card heard meanwhile does not read the body: the room's is newer.
    const read = vi.spyOn(board.workspace.backend, 'card');
    await other.patchCard(board.boardId, cardId, { title: 'Renamed' });
    await settle();
    expect(read).not.toHaveBeenCalled();
    card.close();
    expect(card.room).toBeUndefined();
    await vi.waitFor(() => {
      expect(roomsOf(world).members(cardId)).toBe(0);
    });
  });

  it("puts nothing of the body on the app's undo line: the body's undo is the editor's", async () => {
    const { card, board, cardId, world } = await openAlpha();
    card.openBody();
    await vi.waitFor(() => {
      expect(card.room?.status).toBe('joined');
    });
    const doc = card.room?.doc;
    if (!doc) throw new Error('no document');
    const paragraph = new Y.XmlElement('paragraph');
    paragraph.insert(0, [new Y.XmlText('typed')]);
    doc.getXmlFragment(FIELD).insert(0, [paragraph]);
    await vi.waitFor(() => {
      expect(card.bodyPending).toBe(false);
    });
    roomsOf(world).flush();
    await settle();
    expect(board.workspace.canUndo).toBe(false);
    // Written into the card, and still nothing to undo.
    expect(JSON.stringify(world.blocks.get(cardId)?.body)).toContain('typed');
    card.close();
  });

  it('says when what the reader wrote was dropped with a document made anew', async () => {
    const { card, other, board, cardId, backend } = await openAlpha();
    card.openBody();
    await vi.waitFor(() => {
      expect(card.room?.status).toBe('joined');
    });
    const doc = card.room?.doc;
    if (!doc) throw new Error('no document');
    // The reader's change is on its way when the body is written whole elsewhere.
    backend.latency = 50;
    const paragraph = new Y.XmlElement('paragraph');
    paragraph.insert(0, [new Y.XmlText('mine')]);
    doc.getXmlFragment(FIELD).insert(0, [paragraph]);
    expect(card.bodyPending).toBe(true);
    const read = await other.card(board.boardId, cardId);
    await other.writeCardBody(board.boardId, cardId, {
      body: text('theirs'),
      readAt: read.updateAt,
    });
    await vi.waitFor(() => {
      expect(card.room?.doc).not.toBe(doc);
      expect(card.room?.status).toBe('joined');
    });
    expect(card.lost).toEqual(['document_reset']);
    expect(bodyOf(card.room?.doc ?? doc)).toEqual(text('theirs'));
    card.dismissLost();
    expect(card.lost).toEqual([]);
    card.close();
  });

  it('is missing when its room lets the reader go and the card is no longer theirs to read', async () => {
    const { card, backend, takenOff } = await inTheRoom();
    // Read again, the card is not there for the reader any more, as the server answers.
    const read = vi.spyOn(backend, 'card');
    // The board is read again: its views are read with it, and by nothing else here.
    const boardRead = vi.spyOn(backend, 'views');
    await takenOff();
    await vi.waitFor(() => {
      expect(card.missing).toBe(true);
    });
    expect(card.room?.reason).toBe('not_found');
    await settle();
    // Once for the room put out, however often it says so; and the board is read again.
    expect(read).toHaveBeenCalledTimes(1);
    expect(boardRead).toHaveBeenCalledTimes(1);
    card.close();
  });

  it('asks its room once more when the room lets the reader go but the card can still be read', async () => {
    const statuses: string[] = [];
    const { card, board, cardId, backend, takenOff } = await inTheRoom(statuses);
    // A card that is still read is not called gone; its room, which refuses them now, is not asked again.
    const detail = await backend.card(board.boardId, cardId);
    const read = vi.spyOn(backend, 'card').mockResolvedValue(detail);
    await takenOff();
    const sincePutOut = () => statuses.slice(statuses.indexOf('out'));
    await vi.waitFor(() => {
      expect(sincePutOut()).toEqual(['out', 'joining', 'out']);
    });
    await settle();
    expect(sincePutOut()).toEqual(['out', 'joining', 'out']);
    expect(read).toHaveBeenCalledOnce();
    expect(card.missing).toBe(false);
    expect(card.room?.reason).toBe('not_found');
    card.close();
  });

  it("joins its room again when the card is the reader's again before the room told them", async () => {
    const { card, board, cardId, backend, other, takenOff } = await inTheRoom();
    const me = board.workspace.me?.id ?? '';
    const detail = await backend.card(board.boardId, cardId);
    // Restored (here: let back onto the board) between the room letting them go and the card read.
    const read = vi.spyOn(backend, 'card').mockImplementationOnce(async () => {
      await other.addMember(board.boardId, me, 'editor');
      return detail;
    });
    await takenOff();
    await vi.waitFor(() => {
      expect(read).toHaveBeenCalledOnce();
      expect(card.room?.status).toBe('joined');
    });
    expect(card.missing).toBe(false);
    expect(card.room?.writable).toBe(true);
    card.close();
  });

  it('tells a session that ended when its room lets the reader go', async () => {
    const { card, board, backend, takenOff } = await inTheRoom();
    const ended = new BackendError(401);
    vi.spyOn(backend, 'card').mockRejectedValue(ended);
    const report = vi.spyOn(board.workspace, 'report');
    await takenOff();
    await vi.waitFor(() => {
      expect(report).toHaveBeenCalledWith('load', ended);
    });
    expect(card.missing).toBe(false);
    card.close();
  });

  it('takes no answer about a room it left meanwhile', async () => {
    const { card, backend, takenOff } = await inTheRoom();
    let answer: (error: unknown) => void = () => undefined;
    const read = vi.spyOn(backend, 'card').mockReturnValue(
      new Promise((_, reject) => {
        answer = reject;
      }),
    );
    await takenOff();
    await vi.waitFor(() => {
      expect(read).toHaveBeenCalled();
    });
    // The body is left (the card was deleted and its window made anew) before the answer.
    card.closeBody();
    answer(new BackendError(404));
    await settle();
    expect(card.missing).toBe(false);
    card.close();
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
    const { card, other, board, world } = await openAlpha();
    // Somebody else changes the reader's comment: only an admin may.
    world.members.set(
      board.boardId,
      (world.members.get(board.boardId) ?? []).map((member) =>
        member.userId === other.userId
          ? { ...member, schemeAdmin: true, schemeEditor: false, schemeCommenter: false }
          : member,
      ),
    );
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

  it('uploads an image for the body out of the list', async () => {
    const { card, backend } = await openAlpha();
    backend.latency = 5;
    const uploading = card.uploadFile(new File(['png'], 'a.png', { type: 'image/png' }));
    expect(card.uploads).toHaveLength(0);
    expect(card.filesGoingUp).toBe(true);
    expect(await uploading).toMatch(/./);
    expect(card.filesGoingUp).toBe(false);
  });

  it('gives up an image upload when the card is closed, and tells that once', async () => {
    const { card, backend, workspace } = await openAlpha();
    backend.latency = 5;
    const told = workspace.problems.length;
    const uploading = card.uploadFile(new File(['png'], 'a.png', { type: 'image/png' }));
    const other = card.uploadFile(new File(['%PDF'], 'b.pdf'));
    card.close();
    await expect(uploading).rejects.toThrow();
    await expect(other).rejects.toThrow();
    // One line for what was given up, and none for each request that ended with it.
    expect(workspace.problems.slice(told)).toEqual([
      { id: expect.any(Number) as number, action: 'left', error: undefined },
    ]);
  });

  it('tells nothing of files when a card with none going up is closed', async () => {
    const { card, backend, workspace } = await openAlpha();
    backend.latency = 5;
    await card.uploadFile(new File(['png'], 'a.png', { type: 'image/png' }));
    const told = workspace.problems.length;
    card.close();
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
  it('an admin anyone’s, an editor or a commenter their own, a viewer none', async () => {
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
    expect([board.canDeleteComment(me), board.canDeleteComment('someone')]).toEqual([true, false]);
    // A viewer, and who is no member, none.
    board.members = board.members.map((member) =>
      member.userId === me
        ? { ...member, schemeEditor: false, schemeCommenter: false, schemeViewer: true }
        : member,
    );
    expect(board.role).toBe('viewer');
    expect(board.canDeleteComment(me)).toBe(false);
    as({});
    expect(board.canDeleteComment(me)).toBe(false);
  });
});

describe('who changes a comment', () => {
  it('an admin anyone’s, an editor or a commenter their own, a viewer none', async () => {
    const { board, workspace } = await openAlpha();
    const me = workspace.me?.id ?? '';
    const as = (role: 'schemeAdmin' | 'schemeEditor' | 'schemeCommenter' | 'schemeViewer') => {
      board.members = board.members.map((member) =>
        member.userId === me
          ? {
              ...member,
              schemeAdmin: false,
              schemeEditor: false,
              schemeCommenter: false,
              schemeViewer: false,
              [role]: true,
            }
          : member,
      );
      return [board.canEditComment(me), board.canEditComment('someone')];
    };
    expect(as('schemeAdmin')).toEqual([true, true]);
    expect(as('schemeEditor')).toEqual([true, false]);
    expect(as('schemeCommenter')).toEqual([true, false]);
    expect(as('schemeViewer')).toEqual([false, false]);
    // Any member reads what comments said before; whoever is none (a reader by the link) does not.
    expect(board.canReadCommentHistory).toBe(true);
    board.members = board.members.filter((member) => member.userId !== me);
    expect(board.canReadCommentHistory).toBe(false);
  });
});
