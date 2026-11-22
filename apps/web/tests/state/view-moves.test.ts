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

  it('of two people moving first at once keeps both moves (scene a)', async () => {
    const { board, theirs, other, workspace } = await twoReaders();
    // Nobody moved a view yet: hold Bob's writes until Alice's finish so Bob's `keep`
    // would overwrite Alice's moved view without conditional writes.
    let release = (): void => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const send = other.patchView.bind(other);
    const spy = vi.spyOn(other, 'patchView').mockImplementation(async (boardId, viewId, patch) => {
      await held;
      return send(boardId, viewId, patch);
    });
    const bob = moveView(theirs, idOf(theirs, 'Alpha'), 1);
    await moveView(board, idOf(board, 'Gamma'), -1);
    spy.mockRestore();
    release();
    expect(await bob).toBe(true);
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Gamma', 'Delta']);
    expect(titles(theirs)).toEqual(titles(board));
    expect(workspace.problems).toEqual([]);
    expect(theirs.workspace.problems).toEqual([]);
  });

  it('of a tie raise and a concurrent move of the raised view keeps both moves (scene b)', async () => {
    const { board, theirs, backend, other, workspace } = await twoReaders();
    // Give every view a place with a tie on Beta and Board ('W').
    const setOrder = async (title: string, order: string): Promise<void> => {
      await backend.patchView(board.boardId, idOf(board, title), { updatedFields: { order } });
    };
    await setOrder('Alpha', 'V');
    await setOrder('Board', 'W');
    await setOrder('Beta', 'W');
    await setOrder('Delta', 'Y');
    await setOrder('Gamma', 'Z');
    await settle();
    expect(titles(board)).toEqual(['Alpha', 'Board', 'Beta', 'Delta', 'Gamma']);
    expect(titles(theirs)).toEqual(['Alpha', 'Board', 'Beta', 'Delta', 'Gamma']);

    // Interleaving 1: Alice moves Alpha into the tie (raising Beta), but her raise of Beta
    // arrives AFTER Bob moves Beta down past Delta.
    const betaViewId = idOf(board, 'Beta');
    let releaseRaise = (): void => undefined;
    const holdRaise = new Promise<void>((resolve) => {
      releaseRaise = resolve;
    });
    const aliceSend = backend.patchView.bind(backend);
    const aliceSpy = vi
      .spyOn(backend, 'patchView')
      .mockImplementation(async (boardId, viewId, patch) => {
        if (viewId === betaViewId) await holdRaise;
        return aliceSend(boardId, viewId, patch);
      });
    const alice = moveView(board, idOf(board, 'Alpha'), 1);
    expect(await moveView(theirs, idOf(theirs, 'Beta'), 1)).toBe(true);
    aliceSpy.mockRestore();
    releaseRaise();
    expect(await alice).toBe(true);
    await settle();
    expect(titles(board)).toEqual(['Board', 'Alpha', 'Delta', 'Beta', 'Gamma']);
    expect(titles(theirs)).toEqual(titles(board));
    expect(workspace.problems).toEqual([]);
    expect(theirs.workspace.problems).toEqual([]);

    // Interleaving 2: set up a tie again, Alice raises Beta first, then Bob's move of Beta arrives.
    await setOrder('Alpha', 'V');
    await setOrder('Board', 'W');
    await setOrder('Beta', 'W');
    await setOrder('Delta', 'Y');
    await setOrder('Gamma', 'Z');
    await settle();
    let releaseBob = (): void => undefined;
    const holdBob = new Promise<void>((resolve) => {
      releaseBob = resolve;
    });
    const bobSend = other.patchView.bind(other);
    const bobSpy = vi
      .spyOn(other, 'patchView')
      .mockImplementation(async (boardId, viewId, patch) => {
        await holdBob;
        return bobSend(boardId, viewId, patch);
      });
    const bobMove = moveView(theirs, idOf(theirs, 'Beta'), 1);
    expect(await moveView(board, idOf(board, 'Alpha'), 1)).toBe(true);
    bobSpy.mockRestore();
    releaseBob();
    expect(await bobMove).toBe(true);
    await settle();
    expect(titles(board)).toEqual(['Board', 'Alpha', 'Delta', 'Beta', 'Gamma']);
    expect(titles(theirs)).toEqual(titles(board));
    expect(workspace.problems).toEqual([]);
    expect(theirs.workspace.problems).toEqual([]);
  });

  it('moving the same view twice quickly treats neither move as refused and still reports a refused keep', async () => {
    const { board, backend, workspace } = await openFirstBoard(setup([FIVE]));
    const alpha = idOf(board, 'Alpha');
    const beta = idOf(board, 'Beta');
    const send = backend.patchView.bind(backend);
    vi.spyOn(backend, 'patchView').mockImplementation((boardId, viewId, patch) =>
      viewId === beta
        ? Promise.reject(new BackendError(403, { detail: 'no' }))
        : send(boardId, viewId, patch),
    );
    const first = moveView(board, alpha, 1);
    const second = moveView(board, alpha, 1);
    expect(await Promise.all([first, second])).toEqual([true, true]);
    await settle();
    expect(workspace.problems).toHaveLength(1);
  });

  it('of a first move and a first new view at once keeps both', async () => {
    // Both give the views without a place theirs: conditional writes keep the move either way.
    const added = { title: 'Aardvark', fields: createBoardView().fields };
    const first = await twoReaders();
    await Promise.all([
      moveView(first.board, idOf(first.board, 'Gamma'), -1),
      addViewAtEnd(first.theirs, added),
    ]);
    await settle();
    expect(titles(first.board)).toEqual(['Alpha', 'Beta', 'Board', 'Gamma', 'Delta', 'Aardvark']);
    expect(titles(first.theirs)).toEqual(titles(first.board));
    const second = await twoReaders();
    await Promise.all([
      addViewAtEnd(second.theirs, added),
      moveView(second.board, idOf(second.board, 'Gamma'), -1),
    ]);
    await settle();
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
    // Said once, for the move and the places kept for it: not once a view.
    expect(workspace.problems).toHaveLength(1);
  });

  it('says the places kept for a move that held, refused, once', async () => {
    const { board, backend, workspace } = await twoReaders();
    const beta = idOf(board, 'Beta');
    const send = backend.patchView.bind(backend);
    vi.spyOn(backend, 'patchView').mockImplementation((boardId, viewId, patch) =>
      viewId === beta
        ? send(boardId, viewId, patch)
        : Promise.reject(new BackendError(403, { detail: 'no' })),
    );
    expect(await moveView(board, beta, 1)).toBe(true);
    await settle();
    expect(workspace.problems).toHaveLength(1);
  });

  it('stops after one 409 retry and shows the stored order', async () => {
    const { board, backend, other, workspace } = await twoReaders();
    await moveView(board, idOf(board, 'Gamma'), -1);
    await settle();
    const alpha = idOf(board, 'Alpha');
    // Server changes Alpha before both attempts so every write gets 409.
    const send = backend.patchView.bind(backend);
    let attempts = 0;
    vi.spyOn(backend, 'patchView').mockImplementation(async (boardId, viewId, patch) => {
      attempts += 1;
      await other.patchView(boardId, alpha, { title: `Renamed ${attempts}` });
      return send(boardId, viewId, patch);
    });
    expect(await moveView(board, alpha, 1)).toBe(false);
    // One initial attempt + one retry = 2 total, never looping.
    expect(attempts).toBe(2);
    expect(board.view(alpha)?.title).toBe('Renamed 2');
    expect(titles(board)).toEqual(['Renamed 2', 'Beta', 'Board', 'Gamma', 'Delta']);
    expect(workspace.problems).toHaveLength(1);
  });

  it('does not retry with stale readAt when rereading views after a 409 fails', async () => {
    const { board, backend, world, workspace } = await twoReaders();
    await moveView(board, idOf(board, 'Gamma'), -1);
    await settle();
    const alpha = idOf(board, 'Alpha');
    // Bump stored updateAt without delivering a WS event so board's view is stale.
    const stored = world.blocks.get(alpha);
    if (!stored) throw new Error('missing view');
    stored.updateAt = world.now();
    const viewsSpy = vi
      .spyOn(backend, 'views')
      .mockRejectedValueOnce(new BackendError(503, { detail: 'unavailable' }));
    const patchSpy = vi.spyOn(backend, 'patchView');
    expect(await moveView(board, alpha, 1)).toBe(false);
    expect(viewsSpy).toHaveBeenCalledTimes(1);
    expect(patchSpy).toHaveBeenCalledTimes(1);
    expect(workspace.problems).toHaveLength(1);
  });

  it('recovers rapid double moves and an overlapping rename on a stale view after 409', async () => {
    const { board, backend, world, workspace } = await twoReaders();
    await moveView(board, idOf(board, 'Gamma'), -1);
    await settle();
    const alpha = idOf(board, 'Alpha');
    // Bump stored view without WS delivery so both rapid moves hit 409 first.
    const stored = world.blocks.get(alpha);
    if (!stored) throw new Error('missing view');
    stored.title = 'Alpha remote';
    stored.updateAt = world.now();
    const patchSpy = vi.spyOn(backend, 'patchView');
    const first = moveView(board, alpha, 1);
    const second = moveView(board, alpha, 1);
    expect(await Promise.all([first, second])).toEqual([true, true]);
    await settle();
    // Both initial stale writes failed (2) and both retries succeeded (2) = 4 calls total.
    expect(patchSpy).toHaveBeenCalledTimes(4);
    expect(titles(board)).toEqual(['Beta', 'Board', 'Alpha remote', 'Gamma', 'Delta']);
    expect(workspace.problems).toEqual([]);

    // A stale move followed immediately by an unconditional rename: both survive.
    patchSpy.mockClear();
    const latest = world.blocks.get(alpha);
    if (!latest) throw new Error('missing view');
    latest.title = 'Alpha remote 2';
    latest.updateAt = world.now();
    const moving = moveView(board, alpha, -1);
    const renaming = board.changeView(alpha, (view) => ({ ...view, title: 'Alpha mine' }));
    const [held] = await Promise.all([moving, renaming]);
    expect(held).toBe(true);
    await settle();
    // Stale move (409) + rename (200) + retried move (200) = 3 calls total.
    expect(patchSpy).toHaveBeenCalledTimes(3);
    expect(titles(board)).toEqual(['Beta', 'Alpha mine', 'Board', 'Gamma', 'Delta']);
    expect(workspace.problems).toEqual([]);
  });

  it('retries a stale tie-raise keep once when the raised view was only renamed', async () => {
    const { board, backend, other, workspace } = await twoReaders();
    const setOrder = async (title: string, order: string): Promise<void> => {
      await backend.patchView(board.boardId, idOf(board, title), { updatedFields: { order } });
    };
    await setOrder('Alpha', 'V');
    await setOrder('Board', 'W');
    await setOrder('Beta', 'W');
    await setOrder('Delta', 'Y');
    await setOrder('Gamma', 'Z');
    await settle();
    const betaId = idOf(board, 'Beta');
    const send = backend.patchView.bind(backend);
    let renamed = false;
    vi.spyOn(backend, 'patchView').mockImplementation(async (boardId, viewId, patch) => {
      if (viewId === betaId && !renamed) {
        renamed = true;
        await other.patchView(boardId, betaId, { title: 'Beta renamed' });
      }
      return send(boardId, viewId, patch);
    });
    expect(await moveView(board, idOf(board, 'Alpha'), 1)).toBe(true);
    await settle();
    expect(titles(board)).toEqual(['Board', 'Alpha', 'Beta renamed', 'Delta', 'Gamma']);
    expect(workspace.problems).toEqual([]);
  });

  it('carries readAt on place writes only, never on other view edits', async () => {
    const { board, backend } = await openFirstBoard(setup([FIVE]));
    const alpha = idOf(board, 'Alpha');
    const patch = vi.spyOn(backend, 'patchView');
    await board.setViewFields(alpha, { sortOptions: [{ propertyId: '__title', reversed: true }] });
    await board.setViewFields(alpha, { columnWidths: { __title: 240 } });
    await board.changeView(alpha, (view) => ({ ...view, title: 'Alpha 2' }));
    expect(patch).toHaveBeenCalledTimes(3);
    for (const call of patch.mock.calls) {
      expect(call[2].readAt).toBeUndefined();
    }
    patch.mockClear();
    const readBefore = new Map(board.views.map((view) => [view.id, view.updateAt]));
    await moveView(board, alpha, 1);
    expect(patch.mock.calls.length).toBeGreaterThan(0);
    for (const call of patch.mock.calls) {
      expect(call[2].readAt).toBe(readBefore.get(call[1]));
    }
  });

  it('handles undo and redo with fresh readAt and refuses without overwriting on 409', async () => {
    const { board, backend, other, workspace } = await twoReaders();
    await moveView(board, idOf(board, 'Gamma'), -1);
    await settle();
    const alpha = idOf(board, 'Alpha');
    await moveView(board, alpha, 1);
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Gamma', 'Delta']);

    // An intervening non-order edit and its undo bump updateAt; move undo still works.
    await board.changeView(alpha, (view) => ({ ...view, title: 'Alpha edited' }));
    await workspace.undoLast();
    await workspace.undoLast();
    await settle();
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Gamma', 'Delta']);

    // Redo also uses the fresh updateAt.
    await workspace.redoLast();
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Gamma', 'Delta']);

    // Concurrent move on the server right before undo: undo gets 409, does not overwrite,
    // shows the stored order, reports an undo problem, and leaves the step undoable.
    const send = backend.patchView.bind(backend);
    vi.spyOn(backend, 'patchView').mockImplementationOnce(async (boardId, viewId, patch) => {
      await other.patchView(boardId, alpha, { updatedFields: { order: 'a' } });
      return send(boardId, viewId, patch);
    });
    await workspace.undoLast();
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Board', 'Gamma', 'Delta', 'Alpha']);
    expect(workspace.problems).toHaveLength(1);
    expect(workspace.problems[0]?.action).toBe('undo');
    expect(workspace.canUndo).toBe(true);

    // Asking undo again now uses the refreshed updateAt and succeeds.
    await workspace.undoLast();
    await settle();
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Board', 'Gamma', 'Delta']);

    // Concurrent move right before redo: redo gets 409, does not overwrite, shows stored order.
    vi.spyOn(backend, 'patchView').mockImplementationOnce(async (boardId, viewId, patch) => {
      await other.patchView(boardId, alpha, { updatedFields: { order: 'b' } });
      return send(boardId, viewId, patch);
    });
    await workspace.redoLast();
    await settle();
    expect(titles(board)).toEqual(['Beta', 'Board', 'Gamma', 'Delta', 'Alpha']);
    expect(workspace.problems).toHaveLength(2);
    expect(workspace.problems[1]?.action).toBe('undo');
  });

  it('recognizes a stored place write with readAt when its answer is lost (status 0)', async () => {
    const { board, backend, workspace } = await openFirstBoard(setup([FIVE]));
    await moveView(board, idOf(board, 'Gamma'), -1);
    const alpha = idOf(board, 'Alpha');
    const send = backend.patchView.bind(backend);
    vi.spyOn(backend, 'patchView').mockImplementationOnce(async (boardId, viewId, patch) => {
      await send(boardId, viewId, patch);
      throw new BackendError(0, { detail: 'connection lost' });
    });
    expect(await moveView(board, alpha, 1)).toBe(true);
    expect(titles(board)).toEqual(['Beta', 'Alpha', 'Board', 'Gamma', 'Delta']);
    expect(workspace.problems).toEqual([]);
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

  it('says once that the places kept for a new view or a copy were refused, the view made', async () => {
    const { board, backend, workspace } = await openFirstBoard(setup([FIVE]));
    vi.spyOn(backend, 'patchView').mockRejectedValue(new BackendError(403, { detail: 'no' }));
    const made = await addViewAtEnd(board, {
      title: 'Aardvark',
      fields: createBoardView().fields,
    });
    expect(made).toBeDefined();
    expect(workspace.problems).toHaveLength(1);
    const copy = await duplicateView(board, idOf(board, 'Alpha'), 'Zebra');
    expect(copy).toBeDefined();
    expect(workspace.problems).toHaveLength(2);
  });

  it('says a refusal once when both creating a view and keeping places are refused', async () => {
    const { board, backend, workspace } = await openFirstBoard(setup([FIVE]));
    vi.spyOn(backend, 'createView').mockRejectedValue(new BackendError(403, { detail: 'no' }));
    vi.spyOn(backend, 'patchView').mockRejectedValue(new BackendError(403, { detail: 'no' }));
    expect(
      await addViewAtEnd(board, { title: 'Aardvark', fields: createBoardView().fields }),
    ).toBeUndefined();
    expect(workspace.problems).toHaveLength(1);
    expect(await duplicateView(board, idOf(board, 'Alpha'), 'Zebra')).toBeUndefined();
    expect(workspace.problems).toHaveLength(2);
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
