/** The order of a board's card properties: moved, stored by its own request, followed elsewhere, undone. */
import { describe, expect, it, vi } from 'vitest';

import { ServerBackend } from '$lib/api/backend.ts';
import { movedProperty, withPropertyOrder } from '$lib/logic/property-order.ts';
import { createBoard } from '$lib/logic/wire.ts';
import { BackendError, Workspace } from '$lib/state/index.ts';

import { OWNER, STATUS, openFirstBoard, settle } from './support.ts';

const ids = (board: { cardProperties: { id: string }[] } | undefined): string[] =>
  board?.cardProperties.map((template) => template.id) ?? [];

describe('an order of properties', () => {
  it('moves one a place up or down, and not past an end', () => {
    expect(movedProperty(['a', 'b', 'c'], 'c', -1)).toEqual(['a', 'c', 'b']);
    expect(movedProperty(['a', 'b', 'c'], 'a', 1)).toEqual(['b', 'a', 'c']);
    expect(movedProperty(['a', 'b', 'c'], 'a', -1)).toEqual(['a', 'b', 'c']);
    expect(movedProperty(['a', 'b', 'c'], 'c', 1)).toEqual(['a', 'b', 'c']);
    expect(movedProperty(['a', 'b'], 'x', 1)).toEqual(['a', 'b']);
  });

  it('puts a board in it, keeping one it does not name after the others', () => {
    const board = createBoard();
    board.cardProperties = ['a', 'b', 'c'].map((id) => ({
      id,
      name: id,
      type: 'text',
      options: [],
    }));
    expect(ids(withPropertyOrder(board, ['c', 'a', 'b']))).toEqual(['c', 'a', 'b']);
    expect(ids(withPropertyOrder(board, ['c', 'a']))).toEqual(['c', 'a', 'b']);
    expect(ids(board)).toEqual(['a', 'b', 'c']);
  });
});

describe('the memory backend', () => {
  it('takes an order naming every property once, and refuses any other with nothing changed', async () => {
    const { backend, board } = await openFirstBoard();
    const refused = async (order: string[]) => {
      const error = await backend
        .reorderCardProperties(board.boardId, order)
        .catch((e: unknown) => e);
      return error instanceof BackendError ? error.status : 'taken';
    };
    expect(await refused([OWNER])).toBe(400);
    expect(await refused([OWNER, OWNER])).toBe(400);
    expect(await refused([OWNER, STATUS, 'nothing'])).toBe(400);
    expect(ids(await backend.board(board.boardId))).toEqual([STATUS, OWNER]);
    expect(await backend.reorderCardProperties(board.boardId, [OWNER, STATUS])).toEqual([
      OWNER,
      STATUS,
    ]);
    expect(ids(await backend.board(board.boardId))).toEqual([OWNER, STATUS]);
  });
});

describe('an open board', () => {
  it('shows a new order at once, sends it by itself, and undo puts the old one back', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    const reorder = vi.spyOn(backend, 'reorderCardProperties');
    const patch = vi.spyOn(backend, 'patchBoard');
    const done = board.reorderProperties([OWNER, STATUS]);
    expect(ids(board.board)).toEqual([OWNER, STATUS]);
    await done;
    expect(reorder).toHaveBeenCalledWith(board.boardId, [OWNER, STATUS]);
    expect(patch).not.toHaveBeenCalled();
    expect(ids(await backend.board(board.boardId))).toEqual([OWNER, STATUS]);

    await workspace.undoLast();
    expect(ids(board.board)).toEqual([STATUS, OWNER]);
    expect(ids(await backend.board(board.boardId))).toEqual([STATUS, OWNER]);
  });

  it('puts the order back and tells the reader when it is refused', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    backend.failNext('reorderCardProperties', new BackendError(403));
    await board.reorderProperties([OWNER, STATUS]);
    expect(ids(board.board)).toEqual([STATUS, OWNER]);
    expect(workspace.problems.at(-1)).toMatchObject({ action: 'save', error: { status: 403 } });
  });

  it('follows an order put by somebody else', async () => {
    const made = await openFirstBoard();
    const elsewhere = new Workspace({ backend: made.other, locale: () => 'en' });
    await elsewhere.load();
    const theirs = await elsewhere.openBoard(made.board.boardId);
    await theirs.reorderProperties([OWNER, STATUS]);
    await settle();
    expect(ids(made.board.board)).toEqual([OWNER, STATUS]);
  });
});

describe('the server backend', () => {
  it('puts the order to the board’s property order', async () => {
    const sent: { url: string; method: string; body: unknown }[] = [];
    vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
      sent.push({
        url,
        method: init.method ?? 'GET',
        body: JSON.parse(init.body as string) as unknown,
      });
      return Promise.resolve(
        new Response(JSON.stringify(['b', 'a']), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      );
    });
    expect(await new ServerBackend('').reorderCardProperties('x', ['b', 'a'])).toEqual(['b', 'a']);
    vi.unstubAllGlobals();
    expect(sent).toEqual([
      { url: '/api/v1/boards/x/property-order', method: 'PUT', body: ['b', 'a'] },
    ]);
  });
});
