/**
 * The stores: what a screen reads is sorted here, a change shows at once and
 * is taken back when it is refused, undo takes it back on purpose, and what
 * other people do arrives by itself.
 */
import { describe, expect, it } from 'vitest';

import { BackendError } from '$lib/state/index.ts';
import { droppedCardOrder, placeAmongAll } from '$lib/state/card-order.ts';
import { sidebarOf } from '$lib/state/order.ts';

import { loadTemplates, openFirstBoard, settle, setup, STATUS } from './support.ts';

const refused = (): BackendError => new BackendError(403, { message: 'no' });

describe('the workspace', () => {
  it('reads the reader, the people, the boards and the sidebar', async () => {
    const { workspace } = setup(await loadTemplates());
    await workspace.load();
    expect(workspace.status).toBe('ready');
    expect(workspace.me?.username).toBe('hana');
    expect(workspace.userName(workspace.me?.id ?? '')).toBe('김하나');
    expect(workspace.boards).toHaveLength(13);
    expect(workspace.sidebar.map((entry) => entry.category.name)).toEqual([
      'Team',
      'Research',
      'Personal',
      'Boards',
    ]);
    expect(workspace.sidebar[0]?.boards.map((board) => board.title)).toEqual([
      'Project Tasks',
      'Sprint Planner',
      'Roadmap',
      'Meeting Agenda',
      'Team Retrospective',
    ]);
  });

  it('orders the sidebar itself, whatever order the lists arrived in', () => {
    const category = (id: string, sortOrder: number, boardIds: string[], system = false) => ({
      id,
      name: id,
      userId: 'u',
      teamId: 't',
      type: system ? ('system' as const) : ('custom' as const),
      collapsed: false,
      sortOrder,
      boardIds,
      hiddenBoardIds: [],
      createAt: 0,
      updateAt: 0,
    });
    const board = (id: string, title: string) =>
      ({ id, title, createAt: 0 }) as Parameters<typeof sidebarOf>[1][number];
    const boards = [
      board('b3', 'Zebra'),
      board('b1', 'One'),
      board('b4', 'apple'),
      board('b2', 'Two'),
    ];
    const forwards = sidebarOf(
      [category('second', 10, ['b2', 'gone'], true), category('first', 0, ['b1'])],
      boards,
      'en',
    );
    const backwards = sidebarOf(
      [category('first', 0, ['b1']), category('second', 10, ['b2', 'gone'], true)],
      [...boards].reverse(),
      'en',
    );
    const shape = (sidebar: typeof forwards) =>
      sidebar.map((entry) => [entry.category.id, entry.boards.map((each) => each.id)]);
    expect(shape(forwards)).toEqual([
      ['first', ['b1']],
      // Boards no category names go to the default one, by title; a board that is gone is skipped.
      ['second', ['b2', 'b4', 'b3']],
    ]);
    expect(shape(backwards)).toEqual(shape(forwards));
  });

  it('applies a setting at once and takes it back when the server refuses it', async () => {
    const { workspace, backend } = setup();
    await workspace.load();
    const changing = workspace.changeSettings({ brightness: 'dark' });
    expect(workspace.settings.brightness).toBe('dark');
    await changing;
    expect(workspace.settings.brightness).toBe('dark');

    backend.failNext('patchSettings', new BackendError(400, { field: 'timeZone' }));
    const failing = workspace.changeSettings({ timeZone: 'Asia/Seoul' });
    expect(workspace.settings.timeZone).toBe('Asia/Seoul');
    await failing;
    expect(workspace.settings.timeZone).toBe('auto');
    expect(workspace.problems.at(-1)).toMatchObject({
      action: 'save',
      error: { field: 'timeZone' },
    });
  });

  it('is signed out, with nothing to tell, when the backend says the session is over', async () => {
    const { workspace, backend } = setup();
    backend.failNext('me', new BackendError(401, { message: 'no session' }));
    await workspace.load();
    expect(workspace.status).toBe('signedOut');
    expect(workspace.problems).toEqual([]);

    await workspace.load();
    expect(workspace.status).toBe('ready');
    backend.failNext('patchSettings', new BackendError(401, { message: 'session over' }));
    await workspace.changeSettings({ brightness: 'dark' });
    expect(workspace.status).toBe('signedOut');
    expect(workspace.problems).toEqual([]);
  });

  it('reads the names of people the list of the team did not bring, once', async () => {
    const { workspace } = setup();
    await workspace.load();
    const everyone = workspace.users.map((user) => user.id);
    workspace.users = workspace.users.slice(0, 1);
    await workspace.learnUsers([...everyone, ...everyone]);
    expect(workspace.users.map((user) => user.id).sort()).toEqual([...everyone].sort());
  });

  it('files a board under another category and keeps the order the reader gave', async () => {
    const { workspace } = setup(await loadTemplates());
    await workspace.load();
    const [team, research] = workspace.sidebar;
    const moved = team?.boards[0];
    if (!moved || !research) throw new Error('unexpected seed');
    await workspace.moveBoard(moved.id, research.category.id, 1);
    expect(workspace.sidebar[1]?.boards.map((board) => board.title)[1]).toBe('Project Tasks');
    expect(workspace.sidebar[0]?.boards).toHaveLength(4);
  });
});

describe('making and deleting boards', () => {
  it('makes a board with a first view, a kanban grouped by its first select property', async () => {
    const { workspace, backend } = setup();
    await workspace.load();
    const made = await workspace.createBoard({ title: 'Fresh' }, 'Board view');
    expect(workspace.boards.map((board) => board.title)).toContain('Fresh');
    // A board nobody filed yet shows in the default category.
    expect(workspace.sidebar.at(-1)?.boards.map((board) => board.title)).toContain('Fresh');
    const views = await backend.views(made?.id ?? '');
    expect(views).toHaveLength(1);
    expect(views[0]).toMatchObject({ title: 'Board view', fields: { viewType: 'board' } });
    const status = made?.cardProperties.find((template) => template.type === 'select');
    expect(views[0]?.fields.groupById).toBe(status?.id);
    expect(views[0]?.fields.visibleOptionIds.at(-1)).toBe('');
    const board = await workspace.openBoard(made?.id ?? '');
    expect(board.role).toBe('admin');
  });

  it('deletes a board, and undo brings it back with its cards', async () => {
    const { workspace, backend } = setup();
    await workspace.load();
    const [first] = workspace.boards;
    await workspace.deleteBoard(first?.id ?? '');
    expect(workspace.boards).toHaveLength(0);
    await workspace.undoLast();
    expect(workspace.boards).toHaveLength(1);
    expect(await backend.cards(first?.id ?? '')).toHaveLength(3);
  });
});

describe('a board', () => {
  it('reads its parts and says what the reader may do', async () => {
    const { board } = await openFirstBoard();
    expect(board.status).toBe('ready');
    expect(board.board?.title).toBe('Small');
    expect(board.orderedViews.map((view) => view.title)).toEqual(['Board']);
    expect(board.realCards).toHaveLength(3);
    expect(board.role).toBe('admin');
    expect(board.canEdit).toBe(true);
  });

  it('marks a board that is not there as missing, not as a failure to tell about', async () => {
    const { workspace } = setup();
    await workspace.load();
    const board = await workspace.openBoard('nope');
    expect(board.missing).toBe(true);
    expect(workspace.problems).toHaveLength(0);
  });

  it('knows a board the reader may only look at', async () => {
    const made = setup(await loadTemplates());
    await made.workspace.load();
    const goals = made.workspace.boards.find((board) => board.title.startsWith('Company Goals'));
    const board = await made.workspace.openBoard(goals?.id ?? '');
    expect(board.role).toBe('viewer');
    expect(board.canEdit).toBe(false);
    expect(board.canComment).toBe(false);
  });

  it("shows the cards of a view in the view's order, however the list arrived", async () => {
    const { board } = await openFirstBoard();
    const view = board.orderedViews[0];
    if (!view) throw new Error('no view');
    expect(board.cardsOf(view).map((card) => card.title)).toEqual(['Alpha', 'Beta', 'Gamma']);
    expect(board.cardsOf(view, 'gam').map((card) => card.title)).toEqual(['Gamma']);
    const groups = board.groupsOf(view, board.cardsOf(view), (name) => `No ${name}`);
    expect(groups.visible.map((group) => [group.option.value, group.cards.length])).toEqual([
      ['To do', 2],
      ['Doing', 1],
      ['Done', 0],
      ['No Status', 0],
    ]);
  });
});

describe('changing a card', () => {
  it('shows the change before the backend has answered, then keeps the answer', async () => {
    const { board, backend } = await openFirstBoard();
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    if (!alpha) throw new Error('no card');
    backend.latency = 5;
    const changing = board.setCardProperty(alpha.id, STATUS, 'done');
    expect(board.card(alpha.id)?.fields.properties[STATUS]).toBe('done');
    await changing;
    expect(board.card(alpha.id)?.fields.properties[STATUS]).toBe('done');
    expect(board.card(alpha.id)?.updateAt).toBeGreaterThan(alpha.updateAt);
    expect((await backend.card(board.boardId, alpha.id)).fields.properties[STATUS]).toBe('done');
  });

  it('takes the change back and tells the reader when the backend refuses it', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    if (!alpha) throw new Error('no card');
    backend.failNext('patchCard', refused());
    const changing = board.setCardTitle(alpha.id, 'Renamed');
    expect(board.card(alpha.id)?.title).toBe('Renamed');
    await changing;
    expect(board.card(alpha.id)?.title).toBe('Alpha');
    expect(workspace.problems.at(-1)).toMatchObject({ action: 'save', error: { status: 403 } });
    expect(workspace.canUndo).toBe(false);
  });

  it('keeps the later of two quick changes on screen while the first is answered', async () => {
    const { board, backend } = await openFirstBoard();
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    if (!alpha) throw new Error('no card');
    backend.latency = 5;
    const first = board.setCardTitle(alpha.id, 'One');
    const second = board.setCardProperty(alpha.id, STATUS, 'doing');
    const seen: string[] = [];
    const look = (): void => {
      const card = board.card(alpha.id);
      seen.push(`${card?.title}/${JSON.stringify(card?.fields.properties[STATUS])}`);
    };
    look();
    await first;
    look();
    await second;
    look();
    expect(seen).toEqual(['One/"doing"', 'One/"doing"', 'One/"doing"']);
  });

  it('sends nothing for a change that changes nothing', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    backend.failNext('patchCard', refused());
    await board.setCardTitle(alpha?.id ?? '', 'Alpha');
    expect(workspace.problems).toHaveLength(0);
  });

  it('undoes and redoes', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    if (!alpha) throw new Error('no card');
    await board.setCardTitle(alpha.id, 'Renamed');
    expect(workspace.canUndo).toBe(true);
    await workspace.undoLast();
    expect(board.card(alpha.id)?.title).toBe('Alpha');
    expect((await backend.card(board.boardId, alpha.id)).title).toBe('Alpha');
    expect(workspace.canRedo).toBe(true);
    await workspace.redoLast();
    expect((await backend.card(board.boardId, alpha.id)).title).toBe('Renamed');
  });

  it('creates a card, and undo deletes exactly that one', async () => {
    const { board, workspace } = await openFirstBoard();
    const made = await board.createCard({ title: 'New', properties: { [STATUS]: 'done' } });
    expect(board.realCards.map((card) => card.title)).toContain('New');
    await workspace.undoLast();
    expect(board.card(made?.id)).toBeUndefined();
    await workspace.redoLast();
    expect(board.realCards.filter((card) => card.title === 'New')).toHaveLength(1);
  });

  it('deletes cards, and one undo brings all of them back with their comments', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    const ids = board.realCards.filter((card) => card.title !== 'Gamma').map((card) => card.id);
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    await board.deleteCards(ids);
    expect(board.realCards.map((card) => card.title)).toEqual(['Gamma']);
    await workspace.undoLast();
    expect(board.realCards).toHaveLength(3);
    expect(await backend.comments(board.boardId, alpha?.id ?? '')).toHaveLength(2);
  });
});

describe('dropping cards', () => {
  it('places dropped cards before the card at the place, or after the last of the column', () => {
    const shown = ['a', 'b', 'c', 'd', 'e'];
    expect(droppedCardOrder(shown, ['a', 'b'], ['e'], 0)).toEqual(['e', 'a', 'b', 'c', 'd']);
    expect(droppedCardOrder(shown, ['a', 'b'], ['e'], 1)).toEqual(['a', 'e', 'b', 'c', 'd']);
    expect(droppedCardOrder(shown, ['a', 'b'], ['e'], 2)).toEqual(['a', 'b', 'e', 'c', 'd']);
    expect(droppedCardOrder(shown, ['a', 'b'], ['e'], 99)).toEqual(['a', 'b', 'e', 'c', 'd']);
    expect(droppedCardOrder(shown, [], ['b', 'd'], 0)).toEqual(['a', 'c', 'e', 'b', 'd']);
    expect(droppedCardOrder(shown, ['a', 'c'], ['b'], 1)).toEqual(shown);
  });

  describe('a place among the shown cards, as a place among all of the column', () => {
    it('is the same place when every card of the column is shown', () => {
      const column = ['a', 'b', 'c'];
      for (let index = 0; index <= 3; index += 1) {
        expect(placeAmongAll(column, column, index)).toBe(index);
      }
    });

    it('is right after the shown card above, past the cards a filter hides before it', () => {
      // `b` is hidden: the cards shown are a and c.
      expect(placeAmongAll(['a', 'b', 'c'], ['a', 'c'], 1)).toBe(1);
      // After c, which is the third of the column.
      expect(placeAmongAll(['a', 'b', 'c'], ['a', 'c'], 2)).toBe(3);
      // `a` is hidden: after b, the second.
      expect(placeAmongAll(['a', 'b', 'c'], ['b', 'c'], 1)).toBe(2);
    });

    it('is right before the first shown card when the drop is at the top', () => {
      // `a` is hidden and stays first.
      expect(placeAmongAll(['a', 'b', 'c'], ['b', 'c'], 0)).toBe(1);
    });

    it('skips a shown card the column does not have', () => {
      // `x` is shown in the group (its value is one no option has) but is not counted there.
      expect(placeAmongAll(['a', 'b'], ['a', 'x', 'b'], 2)).toBe(1);
      expect(placeAmongAll(['a', 'b'], ['x', 'a', 'b'], 1)).toBe(0);
      expect(placeAmongAll(['x'], ['stale'], 0)).toBe(1);
    });

    it('is the end of the column when none of it is shown: a folded or filtered-out group', () => {
      expect(placeAmongAll(['a', 'b'], [], 0)).toBe(2);
      expect(placeAmongAll([], [], 0)).toBe(0);
    });

    it('takes a place beyond the shown cards for the end', () => {
      expect(placeAmongAll(['a', 'b'], ['a', 'b'], 99)).toBe(2);
      expect(placeAmongAll(['x', 'a', 'y'], ['a'], Number.POSITIVE_INFINITY)).toBe(2);
    });
  });

  it('counts the cards the page hides when it is told which ones the page shows', async () => {
    const { board } = await openFirstBoard();
    const view = board.orderedViews[0];
    const ids = Object.fromEntries(board.realCards.map((card) => [card.title, card.id]));
    if (!view) throw new Error('no view');
    // The page shows Beta only (a search hides Alpha, which stands before it):
    // Gamma dropped under Beta lands right after Beta, not after the first card.
    await board.dropCards({
      viewId: view.id,
      cardIds: [ids.Gamma ?? ''],
      optionId: 'todo',
      index: 1,
      shown: [ids.Beta ?? ''],
    });
    const current = board.view(view.id);
    if (!current) throw new Error('no view');
    const groups = board.groupsOf(current, board.cardsOf(current), () => 'none');
    expect(groups.visible[0]?.cards.map((card) => card.title)).toEqual(['Alpha', 'Beta', 'Gamma']);
  });

  it('puts a card dropped above the only shown one right before it', async () => {
    const { board } = await openFirstBoard();
    const view = board.orderedViews[0];
    const ids = Object.fromEntries(board.realCards.map((card) => [card.title, card.id]));
    if (!view) throw new Error('no view');
    await board.dropCards({
      viewId: view.id,
      cardIds: [ids.Gamma ?? ''],
      optionId: 'todo',
      index: 0,
      shown: [ids.Beta ?? ''],
    });
    const current = board.view(view.id);
    if (!current) throw new Error('no view');
    const groups = board.groupsOf(current, board.cardsOf(current), () => 'none');
    expect(groups.visible[0]?.cards.map((card) => card.title)).toEqual(['Alpha', 'Gamma', 'Beta']);
  });

  it('moves a card to another column at a place, and one undo takes both changes back', async () => {
    const { board, workspace } = await openFirstBoard();
    const view = board.orderedViews[0];
    const gamma = board.realCards.find((card) => card.title === 'Gamma');
    if (!view || !gamma) throw new Error('unexpected seed');
    await board.dropCards({ viewId: view.id, cardIds: [gamma.id], optionId: 'todo', index: 1 });
    const current = board.view(view.id);
    if (!current) throw new Error('no view');
    const groups = board.groupsOf(current, board.cardsOf(current), () => 'none');
    expect(groups.visible[0]?.cards.map((card) => card.title)).toEqual(['Alpha', 'Gamma', 'Beta']);
    await workspace.undoLast();
    const back = board.view(view.id);
    if (!back) throw new Error('no view');
    const undone = board.groupsOf(back, board.cardsOf(back), () => 'none');
    expect(undone.visible.map((group) => group.cards.map((card) => card.title))).toEqual([
      ['Alpha', 'Beta'],
      ['Gamma'],
      [],
      [],
    ]);
  });

  it('clears the value of a card dropped in the column of cards without one', async () => {
    const { board } = await openFirstBoard();
    const view = board.orderedViews[0];
    const beta = board.realCards.find((card) => card.title === 'Beta');
    if (!view || !beta) throw new Error('unexpected seed');
    await board.dropCards({ viewId: view.id, cardIds: [beta.id], optionId: '' });
    expect(board.card(beta.id)?.fields.properties).not.toHaveProperty(STATUS);
  });
});

describe('changing views and the board', () => {
  it('changes a view at once, reverts a refused change, and undoes', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    const view = board.orderedViews[0];
    if (!view) throw new Error('no view');
    await board.setViewFields(view.id, {
      hiddenOptionIds: ['done'],
      visibleOptionIds: ['todo', 'doing', ''],
    });
    expect(board.view(view.id)?.fields.hiddenOptionIds).toEqual(['done']);
    backend.failNext('patchView', refused());
    await board.changeView(view.id, (current) => ({ ...current, title: 'Renamed' }));
    expect(board.view(view.id)?.title).toBe('Board');
    await workspace.undoLast();
    expect(board.view(view.id)?.fields.hiddenOptionIds).toEqual([]);
  });

  it('creates, copies and deletes views', async () => {
    const { board, workspace } = await openFirstBoard();
    const view = board.orderedViews[0];
    if (!view) throw new Error('no view');
    await board.duplicateView(view.id, 'A copy');
    expect(board.orderedViews.map((each) => each.title)).toEqual(['A copy', 'Board']);
    await board.deleteView(view.id);
    expect(board.orderedViews.map((each) => each.title)).toEqual(['A copy']);
    await workspace.undoLast();
    expect(board.orderedViews.map((each) => each.title)).toEqual(['A copy', 'Board']);
  });

  it('changes the definition of a property', async () => {
    const { board, backend, workspace } = await openFirstBoard();
    await board.changeBoard((current) => ({
      ...current,
      cardProperties: current.cardProperties.map((template) =>
        template.id === STATUS
          ? {
              ...template,
              options: [
                ...template.options,
                { id: 'later', value: 'Later', color: 'propColorGray' },
              ],
            }
          : template,
      ),
    }));
    const stored = await backend.board(board.boardId);
    expect(stored.cardProperties[0]?.options.map((option) => option.id)).toContain('later');
    await workspace.undoLast();
    expect(board.board?.cardProperties[0]?.options).toHaveLength(3);
  });
});

describe('what other people do', () => {
  it('shows up on the board by itself', async () => {
    const { board, other } = await openFirstBoard();
    const beta = board.realCards.find((card) => card.title === 'Beta');
    if (!beta) throw new Error('no card');
    await other.patchCard(board.boardId, beta.id, { title: 'Beta, by someone else' });
    await other.createCard(board.boardId, { title: 'Theirs' });
    await settle();
    expect(board.card(beta.id)?.title).toBe('Beta, by someone else');
    expect(board.realCards.map((card) => card.title)).toContain('Theirs');
    await other.deleteBlock(board.boardId, beta.id);
    await settle();
    expect(board.card(beta.id)).toBeUndefined();
  });

  it('shows a board renamed by someone else in the sidebar too', async () => {
    const { board, other, workspace } = await openFirstBoard();
    await other.patchBoard(board.boardId, { title: 'Renamed elsewhere' });
    await settle();
    expect(board.board?.title).toBe('Renamed elsewhere');
    expect(workspace.sidebar.flatMap((entry) => entry.boards)[0]?.title).toBe('Renamed elsewhere');
  });

  it('stops following a board that was closed', async () => {
    const { board, other, workspace } = await openFirstBoard();
    const beta = board.realCards.find((card) => card.title === 'Beta');
    workspace.closeBoard();
    await other.patchCard(board.boardId, beta?.id ?? '', { title: 'unseen' });
    await settle();
    expect(board.card(beta?.id)?.title).toBe('Beta');
  });
});
