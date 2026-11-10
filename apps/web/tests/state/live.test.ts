/**
 * The server's changes over the WebSocket (`lib/api/live.ts`), against a
 * stand-in socket: what the messages become, to whom they go, and the
 * reading again after the connection comes back.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { LiveChanges, type LiveReads } from '$lib/api/live.ts';
import type { Change } from '$lib/state/index.ts';

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

  drop(): void {
    this.dispatchEvent(new Event('close'));
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
  teamId: () => Promise.resolve('t'),
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
  categories: () => Promise.resolve([]),
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
  it('opens one connection and subscribes to the team', async () => {
    const { socket } = await connected();
    expect(FakeSocket.made).toHaveLength(1);
    expect(socket.url).toBe('ws://here/ws');
    expect(socket.sent).toEqual([{ action: 'SUBSCRIBE_TEAM', teamId: 't' }]);
  });

  it('hands a block to the listeners of its board alone, as the screens’ block', async () => {
    const { socket, board, other, self } = await connected();
    socket.message({
      action: 'UPDATE_BLOCK',
      teamId: 't',
      block: { ...BLOCK, id: 'c1', boardId: 'b1', type: 'card', fields: { properties: {} } },
    });
    socket.message({
      action: 'UPDATE_BLOCK',
      teamId: 't',
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
      teamId: 't',
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
      teamId: 't',
      board: { ...shape, id: 'b1', deleteAt: 0 },
    });
    socket.message({
      action: 'UPDATE_BOARD',
      teamId: 't',
      board: { ...shape, id: 'b1', deleteAt: 9 },
    });
    await settle();
    expect(board.map((change) => change.kind)).toEqual(['board', 'boardDeleted']);
    expect(self.map((change) => change.kind)).toEqual(['board', 'boardDeleted']);
  });

  it('reads all the members of a board when one of them changes', async () => {
    const { socket, board } = await connected();
    socket.message({
      action: 'UPDATE_MEMBER',
      teamId: 't',
      member: { boardId: 'b1', userId: 'u2', schemeViewer: true },
    });
    await settle();
    expect(board).toMatchObject([{ kind: 'members', boardId: 'b1', members: [{ userId: 'u' }] }]);
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
    expect(again?.sent).toEqual([{ action: 'SUBSCRIBE_TEAM', teamId: 't' }]);
    for (const changes of [board, self, other]) expect(changes).toEqual([{ kind: 'resync' }]);
  });

  it('closes the connection when nobody listens any more, and does not open it again', async () => {
    const live = new LiveChanges('ws://here/ws', reads);
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
