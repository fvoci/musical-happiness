/**
 * The trash: the memory backend lists and brings back as the server does,
 * the server backend asks for its pages, and the store of a list keeps its
 * pages and its rows right as things come back.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ServerBackend } from '$lib/api/backend.ts';
import { createBoardView } from '$lib/logic/wire.ts';
import { BackendError, boardTrash, cardTrash, type Change } from '$lib/state/index.ts';

import { openFirstBoard, settle, setup } from './support.ts';

const rejection = async (promise: Promise<unknown>): Promise<BackendError> => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof BackendError) return error;
    throw error;
  }
  throw new Error('the call did not reject');
};

afterEach(() => {
  vi.unstubAllGlobals();
});

async function firstBoard() {
  const made = setup();
  const [team] = await made.backend.teams();
  const [board] = await made.backend.boards(team?.id ?? '');
  if (!board || !team) throw new Error('no board');
  const cards = await made.backend.cards(board.id);
  const id = (title: string): string => cards.find((card) => card.title === title)?.id ?? '';
  const otherId = (await made.other.me()).id;
  return { ...made, teamId: team.id, boardId: board.id, id, otherId };
}

describe('the cards in the trash, in memory', () => {
  it('lists the cards deleted by themselves, the last first, with who and when', async () => {
    const { backend, boardId, id } = await firstBoard();
    const meId = (await backend.me()).id;
    await backend.deleteBlock(boardId, id('Alpha'));
    await backend.deleteBlock(boardId, id('Beta'));
    const listed = await backend.deletedCards(boardId);
    expect(listed.map((item) => item.title)).toEqual(['Beta', 'Alpha']);
    expect(listed.every((item) => item.deletedBy === meId)).toBe(true);
    expect(listed[0]?.deletedAt).toBeGreaterThan(listed[1]?.deletedAt ?? Infinity);
    // A page of one, and the second page.
    expect(
      (await backend.deletedCards(boardId, { page: 1, perPage: 1 })).map((item) => item.title),
    ).toEqual(['Alpha']);
    // A comment deleted is no card of the trash.
    const [comment] = await backend.comments(boardId, id('Gamma'));
    if (comment) await backend.deleteBlock(boardId, comment.id);
    expect(await backend.deletedCards(boardId)).toHaveLength(2);
    await backend.restoreBlock(boardId, id('Alpha'));
    expect((await backend.deletedCards(boardId)).map((item) => item.title)).toEqual(['Beta']);
  });

  it('is for the editors of the board: a commenter or viewer is refused', async () => {
    const { backend, other, boardId, otherId } = await firstBoard();
    await backend.setMemberRole(boardId, otherId, 'commenter');
    expect((await rejection(other.deletedCards(boardId))).status).toBe(403);
    await backend.setMemberRole(boardId, otherId, 'editor');
    expect(await other.deletedCards(boardId)).toEqual([]);
  });
});

describe('the boards in the trash, in memory', () => {
  it('are listed for who was their admin, and brought back by them only', async () => {
    const { backend, other, world, teamId, boardId, otherId } = await firstBoard();
    await backend.setMemberRole(boardId, otherId, 'editor');
    await backend.deleteBoard(boardId);
    expect((await backend.deletedBoards(teamId)).map((item) => item.id)).toEqual([boardId]);
    expect(await other.deletedBoards(teamId)).toEqual([]);
    expect((await rejection(other.restoreBoard(boardId))).status).toBe(403);
    const stranger = backend.as(
      [...world.users.keys()].find(
        (userId) =>
          !(world.deletedBoards.get(boardId)?.members ?? []).some(
            (member) => member.userId === userId,
          ),
      ) ?? '',
    );
    expect((await rejection(stranger.restoreBoard(boardId))).status).toBe(404);
    await backend.restoreBoard(boardId);
    expect(await backend.deletedBoards(teamId)).toEqual([]);
  });

  it('come back where each person had filed them, hidden or not, and the sidebar is told', async () => {
    const { backend, teamId, boardId } = await firstBoard();
    const projects = await backend.createCategory(teamId, { name: 'Projects' });
    await backend.moveBoardToCategory(teamId, boardId, projects.id, 0);
    const second = await backend.createBoard(
      teamId,
      { title: 'Second' },
      { title: 'View', fields: createBoardView().fields },
    );
    await backend.moveBoardToCategory(teamId, second.board.id, projects.id, 1);
    await backend.setBoardHidden(teamId, projects.id, boardId, true);
    const before = (await backend.categories(teamId)).find((one) => one.id === projects.id);
    expect(before?.boardIds).toEqual([boardId, second.board.id]);

    await backend.deleteBoard(boardId);
    const changes: Change[] = [];
    backend.watchSelf((change) => changes.push(change));
    await backend.restoreBoard(boardId);
    await settle();
    const after = (await backend.categories(teamId)).find((one) => one.id === projects.id);
    expect(after?.boardIds).toEqual([boardId, second.board.id]);
    expect(after?.hiddenBoardIds).toEqual([boardId]);
    const told = changes.find((change) => change.kind === 'categories');
    expect(
      told?.kind === 'categories' &&
        told.categories.find((one) => one.id === projects.id)?.boardIds,
    ).toEqual([boardId, second.board.id]);
  });

  it('go to the default category when theirs went meanwhile', async () => {
    const { backend, teamId, boardId } = await firstBoard();
    const projects = await backend.createCategory(teamId, { name: 'Projects' });
    await backend.moveBoardToCategory(teamId, boardId, projects.id, 0);
    await backend.deleteBoard(boardId);
    await backend.deleteCategory(teamId, projects.id);
    await backend.restoreBoard(boardId);
    const system = (await backend.categories(teamId)).find((one) => one.type === 'system');
    expect(system?.boardIds).toContain(boardId);
  });
});

describe('the list of a trash', () => {
  it('reads a page, reads one more, and takes out what comes back', async () => {
    const made = await openFirstBoard();
    const { board, workspace, backend } = made;
    const cards = await backend.cards(board.boardId);
    for (const card of cards) await backend.deleteBlock(board.boardId, card.id);
    const trash = cardTrash(board);
    const spy = vi.spyOn(backend, 'deletedCards');
    await trash.read();
    expect(trash.status).toBe('ready');
    expect(trash.items).toHaveLength(cards.length);
    expect(trash.hasMore).toBe(false);
    const [first] = trash.items;
    expect(await trash.restore(first?.id ?? '')).toBe(true);
    expect(trash.items.map((item) => item.id)).not.toContain(first?.id);
    await settle();
    expect(board.cards.map((card) => card.id)).toContain(first?.id);
    // One that is not in the trash any more (gone meanwhile) leaves the list and is told.
    const [second] = trash.items;
    await backend.restoreBlock(board.boardId, second?.id ?? '');
    expect(await trash.restore(second?.id ?? '')).toBe(false);
    expect(trash.items.map((item) => item.id)).not.toContain(second?.id);
    expect(workspace.problems.length).toBeGreaterThan(0);
    expect(spy).toHaveBeenCalledWith(board.boardId, { page: 0, perPage: 50 });
  });

  it('reads again the whole list one page longer for more, so that nothing is skipped', async () => {
    const made = await openFirstBoard();
    const { workspace } = made;
    const items = Array.from({ length: 75 }, (_, at) => ({
      id: `b${at}`,
      title: `Board ${at}`,
      deletedAt: 1000 - at,
      deletedBy: '',
    }));
    const asked: unknown[] = [];
    vi.spyOn(workspace.backend, 'deletedBoards').mockImplementation((_teamId, paging) => {
      asked.push(paging);
      return Promise.resolve(items.slice(0, paging?.perPage ?? 100));
    });
    const trash = boardTrash(workspace);
    await trash.read();
    expect(trash.items).toHaveLength(50);
    expect(trash.hasMore).toBe(true);
    await trash.more();
    expect(trash.items).toHaveLength(75);
    expect(trash.hasMore).toBe(false);
    expect(asked).toEqual([
      { page: 0, perPage: 50 },
      { page: 0, perPage: 100 },
    ]);
  });

  it('says a list that could not be read failed', async () => {
    const { workspace } = await openFirstBoard();
    vi.spyOn(workspace.backend, 'deletedBoards').mockRejectedValue(new BackendError(500));
    const trash = boardTrash(workspace);
    await trash.read();
    expect(trash.status).toBe('failed');
    expect(trash.items).toEqual([]);
  });
});

describe('the trash, on the server', () => {
  it('asks for the pages of cards and boards, and brings a board back', async () => {
    const sent: string[] = [];
    vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
      sent.push(`${init.method ?? 'GET'} ${url}`);
      const item = { id: 'x', title: 'X', deletedAt: 5, deletedBy: 'u' };
      return Promise.resolve(
        new Response(JSON.stringify([item]), { headers: { 'Content-Type': 'application/json' } }),
      );
    });
    const backend = new ServerBackend('');
    expect(await backend.deletedCards('b1', { page: 2, perPage: 50 })).toEqual([
      { id: 'x', title: 'X', deletedAt: 5, deletedBy: 'u' },
    ]);
    await backend.deletedBoards('t1');
    expect(sent).toEqual([
      'GET /api/v1/boards/b1/trash/cards?page=2&perPage=50',
      'GET /api/v1/teams/t1/trash/boards',
    ]);
  });
});
