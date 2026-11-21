/**
 * The order of a board's views: each view's place in its own `fields.order`,
 * read before the views without one (by title); moved a place up or down,
 * written for the view moved alone once the board's views have places, and
 * undone without taking back what keeps the others where they show; followed
 * by somebody else; a new view at the end; carried by a copy of the view, of
 * the board, and through a template.
 */
import { describe, expect, it, vi } from 'vitest';

import { createBoardView, type BoardView } from '$lib/logic/wire.ts';
import {
  addViewAtEnd,
  BackendError,
  duplicateView,
  moveView,
  Workspace,
  type BoardStore,
} from '$lib/state/index.ts';
import type { SeedTemplate } from '$lib/state/memory/seed.ts';
import { orderViews } from '$lib/state/order.ts';

import { openFirstBoard, settle, setup, SMALL, type Setup } from './support.ts';

/** The small board with five views, nobody having moved one: Board, and four more. */
const FIVE: SeedTemplate = {
  ...SMALL,
  views: [
    ...SMALL.views,
    ...['Delta', 'Alpha', 'Gamma', 'Beta'].map((title) => ({
      ...SMALL.views[0],
      id: title.toLowerCase(),
      title,
    })),
  ],
} as SeedTemplate;

const titles = (board: BoardStore): string[] => board.orderedViews.map((view) => view.title);
const idOf = (board: BoardStore, title: string): string =>
  board.views.find((view) => view.title === title)?.id ?? '';
const places = (board: BoardStore): (string | undefined)[] =>
  board.orderedViews.map((view) => view.fields.order);

/** The board open for the reader, and for somebody else who may change it (an editor). */
async function twoReaders(): Promise<Setup & { board: BoardStore; theirs: BoardStore }> {
  const made = await openFirstBoard(setup([FIVE]));
  made.world.members.set(
    made.board.boardId,
    (made.world.members.get(made.board.boardId) ?? []).map((member) =>
      member.userId === made.other.userId
        ? { ...member, schemeEditor: true, schemeCommenter: true }
        : member,
    ),
  );
  const elsewhere = new Workspace({ backend: made.other, locale: () => 'en' });
  await elsewhere.load();
  const theirs = await elsewhere.openBoard(made.board.boardId);
  return { ...made, theirs };
}

describe('reading the order', () => {
  const view = (id: string, title: string, order?: unknown): BoardView => {
    const made = createBoardView();
    return {
      ...made,
      id,
      title,
      fields: { ...made.fields, ...(order === undefined ? {} : { order: order as string }) },
    };
  };

  it('lists the views with a place by it, then those without one by title', () => {
    const ordered = orderViews(
      [
        view('1', 'Zulu'),
        view('2', 'Echo', 'X'),
        view('3', 'Alpha'),
        view('4', 'Bravo', 'V'),
        // Two of one place (moved there at once): the identifier settles it, everywhere.
        view('6', 'Lima', 'W'),
        view('5', 'Kilo', 'W'),
        // What is no place counts as none.
        view('7', 'Mike', 'V0'),
        view('8', 'Charlie', 7),
      ],
      'en',
    );
    expect(ordered.map((each) => each.title)).toEqual([
      'Bravo',
      'Kilo',
      'Lima',
      'Echo',
      'Alpha',
      'Charlie',
      'Mike',
      'Zulu',
    ]);
  });
});

describe('a move', () => {
  it('gives every view the place it shows at the first time; undo puts back the one moved', async () => {
    const { board, backend, workspace } = await openFirstBoard(setup([FIVE]));
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Delta', 'Gamma']);
    const patch = vi.spyOn(backend, 'patchView');
    const done = moveView(board, idOf(board, 'Delta'), -1);
    // Shown at once.
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Delta', 'Board', 'Gamma']);
    await done;
    // Every view the place it showed at, and the one moved where it goes.
    expect(patch).toHaveBeenCalledTimes(6);
    expect(places(board).every((place) => place !== undefined)).toBe(true);
    const stored = (await backend.views(board.boardId)).map((view) => view.fields.order);
    expect(stored.every((place) => place !== undefined)).toBe(true);

    // One write back: the view moved, where it showed. The others keep their places.
    await workspace.undoLast();
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Delta', 'Gamma']);
    expect(patch).toHaveBeenCalledTimes(7);
    expect(places(board).every((place) => place !== undefined)).toBe(true);
    const read = orderViews(await backend.views(board.boardId), 'en');
    expect(read.map((view) => view.title)).toEqual(titles(board));
  });

  it('undone after somebody else added a view, leaves that view at the end', async () => {
    const { board, theirs, workspace } = await twoReaders();
    await moveView(board, idOf(board, 'Delta'), -1);
    await settle();
    await addViewAtEnd(theirs, { title: 'Aardvark', fields: createBoardView().fields });
    await settle();
    expect(titles(board).at(-1)).toBe('Aardvark');
    await workspace.undoLast();
    await settle();
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Delta', 'Gamma', 'Aardvark']);
    expect(titles(theirs)).toEqual(titles(board));
  });

  it('writes the view moved alone once the views have places, and not past an end', async () => {
    const { board, backend } = await openFirstBoard(setup([FIVE]));
    await moveView(board, idOf(board, 'Gamma'), -1);
    const patch = vi.spyOn(backend, 'patchView');
    await moveView(board, idOf(board, 'Alpha'), 1);
    expect(patch).toHaveBeenCalledTimes(1);
    expect(patch.mock.calls[0]?.[1]).toBe(idOf(board, 'Alpha'));
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Gamma', 'Delta']);
    await moveView(board, idOf(board, 'Beta'), -1);
    await moveView(board, idOf(board, 'Delta'), 1);
    expect(patch).toHaveBeenCalledTimes(1);
  });

  it('is seen by somebody else, and kept by the backend', async () => {
    const { board, theirs, backend } = await twoReaders();
    await moveView(board, idOf(board, 'Gamma'), -1);
    await settle();
    expect(titles(theirs)).toEqual(['Alpha', 'Beta', 'Board', 'Gamma', 'Delta']);
    const read = orderViews(await backend.views(board.boardId), 'en');
    expect(read.map((view) => view.title)).toEqual(titles(board));
  });

  it('of two people moving two views at once keeps both moves', async () => {
    const { board, theirs } = await twoReaders();
    // The views have places (the board's first move, long done).
    await moveView(board, idOf(board, 'Gamma'), -1);
    await settle();
    // Both from the same list: Alpha, Beta, Board, Gamma, Delta.
    const mine = moveView(board, idOf(board, 'Delta'), -1);
    const other = moveView(theirs, idOf(theirs, 'Alpha'), 1);
    await Promise.all([mine, other]);
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Delta', 'Gamma']);
    expect(titles(theirs)).toEqual(titles(board));
  });

  it('of two people moving at once into one gap puts them by identifier, alike for both', async () => {
    const { board, theirs } = await twoReaders();
    await moveView(board, idOf(board, 'Gamma'), -1);
    await settle();
    // Alpha, Beta, Board, Gamma, Delta: one from above, one from below, between Beta and Board.
    await Promise.all([
      moveView(board, idOf(board, 'Alpha'), 1),
      moveView(theirs, idOf(theirs, 'Gamma'), -1),
    ]);
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Gamma', 'Board', 'Delta']);
    expect(titles(theirs)).toEqual(titles(board));
    // A move between the two finds no room there: the one of the two after it goes up,
    // past where Board is, whose place is moved up with it first; nothing else is written.
    const patch = vi.spyOn(board.workspace.backend, 'patchView');
    await moveView(board, idOf(board, 'Board'), -1);
    await settle();
    expect(patch.mock.calls.map((call) => call[1]).sort()).toEqual(
      [idOf(board, 'Board'), idOf(board, 'Board'), idOf(board, 'Gamma')].sort(),
    );
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Gamma', 'Delta']);
    expect(titles(theirs)).toEqual(titles(board));
    expect(new Set(places(board)).size).toBe(5);
    // The move is a step of undo: undone, Board is back after Gamma, which stays raised.
    await board.workspace.undoLast();
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Gamma', 'Board', 'Delta']);
    expect(titles(theirs)).toEqual(titles(board));
  });

  it('of two people moving first at once keeps the moves whose writes came last', async () => {
    const { board, theirs } = await twoReaders();
    // Nobody moved a view yet: both give every view a place, the same for a view neither
    // moves; the view one of them moved is where the later write puts it.
    const mine = moveView(board, idOf(board, 'Gamma'), -1);
    const other = moveView(theirs, idOf(theirs, 'Alpha'), 1);
    await Promise.all([mine, other]);
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Delta', 'Gamma']);
    expect(titles(theirs)).toEqual(titles(board));
  });

  it('of a first move and a first new view at once keeps where the later write put the view', async () => {
    // Both give the views without a place theirs: the same places, but for the view moved,
    // which the move writes where it goes and the new view's writes where it showed.
    const added = { title: 'Aardvark', fields: createBoardView().fields };
    const first = await twoReaders();
    await Promise.all([
      moveView(first.board, idOf(first.board, 'Gamma'), -1),
      addViewAtEnd(first.theirs, added),
    ]);
    await settle();
    // The new view's writes came later: Gamma is where it showed, the move gone.
    expect(titles(first.board)).toEqual(['Alpha', 'Beta', 'Board', 'Delta', 'Gamma', 'Aardvark']);
    expect(titles(first.theirs)).toEqual(titles(first.board));
    const second = await twoReaders();
    await Promise.all([
      addViewAtEnd(second.theirs, added),
      moveView(second.board, idOf(second.board, 'Gamma'), -1),
    ]);
    await settle();
    // The move's came later: it stays.
    expect(titles(second.board)).toEqual(['Alpha', 'Beta', 'Board', 'Gamma', 'Delta', 'Aardvark']);
    expect(titles(second.theirs)).toEqual(titles(second.board));
  });

  it('refused, shows the view moved as it is stored, as every browser reads it', async () => {
    const { board, backend, theirs, workspace } = await twoReaders();
    const beta = idOf(board, 'Beta');
    const send = backend.patchView.bind(backend);
    vi.spyOn(backend, 'patchView').mockImplementation((boardId, viewId, patch) =>
      viewId === beta
        ? Promise.reject(new BackendError(403, { detail: 'no' }))
        : send(boardId, viewId, patch),
    );
    await moveView(board, beta, 1);
    await settle();
    // The others have the places they showed at; Beta has none, and so reads last.
    expect(titles(board)).toEqual(['Alpha', 'Board', 'Delta', 'Gamma', 'Beta']);
    expect(titles(theirs)).toEqual(titles(board));
    // Said once for the places kept, once for the move: not once a view.
    expect(workspace.problems).toHaveLength(2);
  });
});

describe('a new view and copies', () => {
  it('goes at the end, the views without a place given theirs the first time', async () => {
    const { board, backend, workspace } = await openFirstBoard(setup([FIVE]));
    const patch = vi.spyOn(backend, 'patchView');
    const made = await addViewAtEnd(board, {
      title: 'Aardvark',
      fields: createBoardView().fields,
    });
    expect(made?.fields.order).toBeDefined();
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Delta', 'Gamma', 'Aardvark']);
    // The five, once each, the places they showed at.
    expect(patch).toHaveBeenCalledTimes(5);
    // Once they have places, a view added is its making alone.
    patch.mockClear();
    await addViewAtEnd(board, { title: 'Aardwolf', fields: createBoardView().fields });
    expect(patch).not.toHaveBeenCalled();
    expect(titles(board).slice(-2)).toEqual(['Aardvark', 'Aardwolf']);
    // Undone, the view goes; the others stay where they show.
    await workspace.undoLast();
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Delta', 'Gamma', 'Aardvark']);
  });

  it('puts a copy of a view right after it, whether the views have places or not', async () => {
    const { board } = await openFirstBoard(setup([FIVE]));
    await duplicateView(board, idOf(board, 'Delta'), 'Copy of Delta');
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Delta', 'Copy of Delta', 'Gamma']);
    expect(board.views.every((view) => view.fields.order !== undefined)).toBe(true);
    await duplicateView(board, idOf(board, 'Alpha'), 'Zebra');
    expect(titles(board).slice(0, 2)).toEqual(['Alpha', 'Zebra']);
  });

  it('keeps the order in a copy of the board, and through a template', async () => {
    const { board, backend, workspace, world } = await openFirstBoard(setup([FIVE]));
    await moveView(board, idOf(board, 'Gamma'), -1);
    await moveView(board, idOf(board, 'Gamma'), -1);
    await moveView(board, idOf(board, 'Board'), 1);
    const order = titles(board);
    expect(order).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta', 'Board']);
    const read = async (boardId: string): Promise<string[]> =>
      orderViews(await backend.views(boardId), 'en').map((view) => view.title);
    const copy = await backend.duplicateContainer(board.boardId, false, world.workspace.id);
    expect(await read(copy.id)).toEqual(order);
    const template = await backend.duplicateContainer(board.boardId, true, world.workspace.id);
    const fromTemplate = await backend.duplicateContainer(template.id, false, world.workspace.id);
    expect(await read(fromTemplate.id)).toEqual(order);
    expect(workspace.problems).toEqual([]);
  });
});
