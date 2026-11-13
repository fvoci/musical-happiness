/**
 * The server's changes over the WebSocket (`lib/api/live.ts`), against a
 * stand-in socket: what the messages become, to whom they go, and the
 * reading again after the connection comes back.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { LiveChanges, type LiveReads } from '$lib/api/live.ts';
import { BackendError, type Change } from '$lib/state/index.ts';

class FakeSocket extends EventTarget {
  static made: FakeSocket[] = [];
  readonly sent: unknown[] = [];
  closed = false;
  readonly url: string;

  constructor(url: string) {
    super();
    this.url = url;
    FakeSocket.made.push(this);
  }

  send(text: string): void {
    this.sent.push(JSON.parse(text));
  }

  close(): void {
    this.closed = true;
  }

  open(): void {
    this.dispatchEvent(new Event('open'));
  }

  message(data: unknown): void {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(data) }));
  }

  drop(code = 1006): void {
    this.dispatchEvent(new CloseEvent('close', { code }));
  }
}

const BLOCK = {
  parentId: 'b1',
  createAt: 1,
  updateAt: 2,
  deleteAt: 0,
  createdBy: 'u',
  modifiedBy: 'u',
  schema: 1,
  title: 'A card',
};

const reads: LiveReads = {
  members: (boardId) =>
    Promise.resolve([
      {
        boardId,
        userId: 'u',
        minimumRole: '',
        schemeAdmin: true,
        schemeEditor: false,
        schemeCommenter: false,
        schemeViewer: false,
        synthetic: false,
      },
    ]),
};

/** Lets the promises that are due run. */
const settle = async (): Promise<void> => {
  for (let turn = 0; turn < 5; turn += 1) await Promise.resolve();
};

beforeEach(() => {
  FakeSocket.made = [];
  vi.stubGlobal('WebSocket', FakeSocket);
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function connected(): Promise<{
  live: LiveChanges;
  board: Change[];
  self: Change[];
  other: Change[];
  socket: FakeSocket;
}> {
  const live = new LiveChanges('ws://here/ws', reads);
  live.enter('t');
  const board: Change[] = [];
  const self: Change[] = [];
  const other: Change[] = [];
  live.watch('b1', (change) => board.push(change));
  live.watch('b2', (change) => other.push(change));
  live.watch(undefined, (change) => self.push(change));
  await settle();
  const socket = FakeSocket.made[0];
  if (!socket) throw new Error('no socket');
  socket.open();
  return { live, board, self, other, socket };
}

describe('changes from the server', () => {
  it('opens one connection and subscribes to the workspace', async () => {
    const { socket } = await connected();
    expect(FakeSocket.made).toHaveLength(1);
    expect(socket.url).toBe('ws://here/ws');
    expect(socket.sent).toEqual([{ action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' }]);
  });

  it('hands a block to the listeners of its board alone, as the screens’ block', async () => {
    const { socket, board, other, self } = await connected();
    socket.message({
      action: 'UPDATE_BLOCK',
      workspaceId: 't',
      block: { ...BLOCK, id: 'c1', boardId: 'b1', type: 'card', fields: { properties: {} } },
    });
    socket.message({
      action: 'UPDATE_BLOCK',
      workspaceId: 't',
      block: { ...BLOCK, id: 'c1', boardId: 'b1', deleteAt: 5, type: '', fields: {} },
    });
    await settle();
    expect(board).toMatchObject([
      { kind: 'blocks', boardId: 'b1', blocks: [{ id: 'c1', type: 'card', title: 'A card' }] },
      { kind: 'blocksDeleted', boardId: 'b1', blockIds: ['c1'] },
    ]);
    expect(other).toEqual([]);
    expect(self).toEqual([]);
  });

  it('hands a board to its listeners and to the reader’s own; a deleted one as deleted', async () => {
    const { socket, board, self } = await connected();
    const shape = {
      workspaceId: 't',
      channelId: '',
      createdBy: 'u',
      modifiedBy: 'u',
      type: 'P',
      minimumRole: '',
      title: 'B',
      description: '',
      icon: '',
      showDescription: false,
      isTemplate: false,
      templateVersion: 0,
      properties: {},
      cardProperties: [],
      createAt: 1,
      updateAt: 2,
    };
    socket.message({
      action: 'UPDATE_BOARD',
      workspaceId: 't',
      board: { ...shape, id: 'b1', deleteAt: 0 },
    });
    socket.message({
      action: 'UPDATE_BOARD',
      workspaceId: 't',
      board: { ...shape, id: 'b1', deleteAt: 9 },
    });
    await settle();
    expect(board.map((change) => change.kind)).toEqual(['board', 'boardDeleted']);
    expect(self.map((change) => change.kind)).toEqual(['board', 'boardDeleted']);
  });

  it('reads all the members of a watched board when one changes, and the sidebar soon after', async () => {
    const members = vi.spyOn(reads, 'members');
    const { socket, board, self } = await connected();
    socket.message({
      action: 'UPDATE_MEMBER',
      workspaceId: 't',
      member: { boardId: 'b1', userId: 'u2', schemeViewer: true },
    });
    // A board nobody here watches: its members are not read.
    socket.message({
      action: 'DELETE_MEMBER',
      workspaceId: 't',
      member: { boardId: 'b9', userId: 'u2' },
    });
    await settle();
    expect(board).toMatchObject([{ kind: 'members', boardId: 'b1', members: [{ userId: 'u' }] }]);
    expect(members.mock.calls).toEqual([['b1']]);
    // Being added to a board, or taken off one, shows in the sidebar: read once for both.
    expect(self).toEqual([]);
    await vi.advanceTimersByTimeAsync(200);
    expect(self).toEqual([{ kind: 'resync' }]);
    members.mockRestore();
  });

  it('tells only the later of two readings of members, and a board taken away as gone', async () => {
    let answer: (value: Awaited<ReturnType<LiveReads['members']>>) => void = () => undefined;
    const members = vi
      .spyOn(reads, 'members')
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            answer = resolve;
          }),
      )
      .mockImplementationOnce(() => Promise.reject(new BackendError(403, { detail: 'no' })));
    const { socket, board } = await connected();
    const message = {
      action: 'UPDATE_MEMBER',
      workspaceId: 't',
      member: { boardId: 'b1', userId: 'u2' },
    };
    socket.message(message);
    socket.message(message);
    await settle();
    answer([]);
    await settle();
    expect(board).toEqual([{ kind: 'boardDeleted', boardId: 'b1' }]);
    members.mockRestore();
  });

  it('reads the sidebar again on a message of the categories', async () => {
    const { socket, self, board } = await connected();
    socket.message({ action: 'UPDATE_CATEGORY', workspaceId: 't', category: {} });
    socket.message({ action: 'REORDER_CATEGORIES', workspaceId: 't', categoryOrder: [] });
    await vi.advanceTimersByTimeAsync(200);
    expect(self).toEqual([{ kind: 'resync' }]);
    expect(board).toEqual([]);
  });

  it('asks again for a workspace whose subscription the server ended, and has everyone read again', async () => {
    const { socket, board, self } = await connected();
    socket.message({
      action: 'UNSUBSCRIBED_WORKSPACE',
      workspaceId: 't',
      reason: 'workspace access ended',
    });
    socket.message({
      action: 'UNSUBSCRIBED_WORKSPACE',
      workspaceId: 'left',
      reason: 'workspace access ended',
    });
    await settle();
    expect(socket.closed).toBe(false);
    expect(socket.sent).toEqual([
      { action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' },
      { action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' },
    ]);
    expect(board).toEqual([{ kind: 'resync' }]);
    expect(self).toEqual([{ kind: 'resync' }]);
  });

  it('opens again at once after signing in, and has everyone read again', async () => {
    const { live, socket, board } = await connected();
    live.restart();
    expect(socket.closed).toBe(true);
    await settle();
    const again = FakeSocket.made[1];
    again?.open();
    expect(again?.sent).toEqual([{ action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' }]);
    expect(board).toEqual([{ kind: 'resync' }]);
  });

  it('opens nothing until a workspace is entered, and then subscribes to it', async () => {
    const live = new LiveChanges('ws://here/ws', reads);
    live.watch(undefined, () => undefined);
    await settle();
    expect(FakeSocket.made).toHaveLength(0);
    live.enter('t');
    FakeSocket.made[0]?.open();
    expect(FakeSocket.made[0]?.sent).toEqual([{ action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' }]);
  });

  it('moves the subscription to another workspace, and lets go of what the old one still sends', async () => {
    const { live, socket, self } = await connected();
    live.enter('t2');
    live.enter('t2');
    expect(FakeSocket.made).toHaveLength(1);
    expect(socket.sent).toEqual([
      { action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' },
      { action: 'UNSUBSCRIBE_WORKSPACE', workspaceId: 't' },
      { action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't2' },
    ]);
    const board = (workspaceId: string, id: string): unknown => ({
      action: 'UPDATE_BOARD',
      workspaceId,
      board: { id, workspaceId, deleteAt: 0, title: id, cardProperties: [], properties: {} },
    });
    socket.message(board('t', 'old'));
    socket.message(board('t2', 'new'));
    await settle();
    expect(self.map((change) => (change.kind === 'board' ? change.board.id : change.kind))).toEqual(
      ['new'],
    );
  });

  it('subscribes a connection that opens after entering to the workspace entered last', async () => {
    const live = new LiveChanges('ws://here/ws', reads);
    live.enter('t');
    live.watch('b1', () => undefined);
    live.enter('t2');
    FakeSocket.made[0]?.open();
    expect(FakeSocket.made).toHaveLength(1);
    expect(FakeSocket.made[0]?.sent).toEqual([
      { action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't2' },
    ]);
  });

  it('opens again at once on signing in, subscribed to the workspace entered', async () => {
    const { live, socket } = await connected();
    live.restart();
    expect(socket.closed).toBe(true);
    FakeSocket.made[1]?.open();
    expect(FakeSocket.made[1]?.sent).toEqual([{ action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' }]);
  });

  it('waits from the first wait again only after a connection has held', async () => {
    const { socket } = await connected();
    socket.drop();
    await vi.advanceTimersByTimeAsync(500);
    // Opens, and is closed at once: the next wait is the longer one.
    FakeSocket.made[1]?.open();
    FakeSocket.made[1]?.drop();
    await vi.advanceTimersByTimeAsync(999);
    expect(FakeSocket.made).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeSocket.made).toHaveLength(3);
    // This one holds; a drop after it waits the first wait.
    FakeSocket.made[2]?.open();
    await vi.advanceTimersByTimeAsync(10_000);
    FakeSocket.made[2]?.drop();
    await vi.advanceTimersByTimeAsync(500);
    expect(FakeSocket.made).toHaveLength(4);
  });

  it('opens again after a drop, waiting longer each time, and has everyone read again', async () => {
    const { socket, board, self, other } = await connected();
    socket.drop();
    await vi.advanceTimersByTimeAsync(499);
    expect(FakeSocket.made).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeSocket.made).toHaveLength(2);
    FakeSocket.made[1]?.drop();
    await vi.advanceTimersByTimeAsync(999);
    expect(FakeSocket.made).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    const again = FakeSocket.made[2];
    again?.open();
    expect(again?.sent).toEqual([{ action: 'SUBSCRIBE_WORKSPACE', workspaceId: 't' }]);
    for (const changes of [board, self, other]) expect(changes).toEqual([{ kind: 'resync' }]);
  });

  it('spreads the wait after the server asks to try again, so its pages do not all come back at once', async () => {
    const random = vi.spyOn(Math, 'random').mockReturnValue(0.5);
    try {
      const { socket } = await connected();
      socket.drop(1013);
      await vi.advanceTimersByTimeAsync(749);
      expect(FakeSocket.made).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(FakeSocket.made).toHaveLength(2);
    } finally {
      random.mockRestore();
    }
  });

  it('closes the connection when nobody listens any more, and does not open it again', async () => {
    const live = new LiveChanges('ws://here/ws', reads);
    live.enter('t');
    const stop = live.watch('b1', () => undefined);
    await settle();
    const socket = FakeSocket.made[0];
    socket?.open();
    stop();
    expect(socket?.closed).toBe(true);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(FakeSocket.made).toHaveLength(1);
  });
});
