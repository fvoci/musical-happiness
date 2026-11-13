/**
 * A body opened in its room (`Backend.openDoc`): two readers of the backend
 * in memory edit one body together and the card's body follows, a reader
 * who may only read is let in to read, and the page's connection
 * (`api/live.ts` with `live-rooms.ts`) carries the room's messages and
 * frames to the room's client and back.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as Y from 'yjs';

import { LiveChanges } from '$lib/api/live.ts';
import { decodeDocFrame, encodeDocFrame, SYNC_STEP_2, SYNC_UPDATE } from '$lib/logic/doc-frame.ts';
import type { BodyDoc, BodyDocHears, BodyDocState, Change } from '$lib/state/backend.ts';
import { openRoom } from '$lib/state/doc-client.ts';
import { bodyOf, FIELD, roomsOf } from '$lib/state/memory/rooms.ts';

import { setup } from './support.ts';

/** What an opener of a body heard. */
function listener(): BodyDocHears & { states: BodyDocState[]; lost: string[] } {
  const states: BodyDocState[] = [];
  const lost: string[] = [];
  return {
    states,
    lost,
    state: (state) => states.push(state),
    discarded: (why) => lost.push(why),
  };
}

/** Writes a paragraph of `value` at the end of a body's document. */
function append(doc: Y.Doc, value: string): void {
  const paragraph = new Y.XmlElement('paragraph');
  paragraph.insert(0, [new Y.XmlText(value)]);
  const fragment = doc.getXmlFragment(FIELD);
  fragment.insert(fragment.length, [paragraph]);
}

const texts = (doc: Y.Doc | undefined): string[] =>
  doc
    ? doc
        .getXmlFragment(FIELD)
        .toArray()
        .map((node) => node.toJSON() as string)
    : [];

/** The world in memory, its first board, a card of it with a body, and two of its members. */
function world(): ReturnType<typeof setup> & { boardId: string; cardId: string } {
  const made = setup();
  const card = [...made.world.blocks.values()].find((block) => block.type === 'card' && block.body);
  if (!card) throw new Error('the seed has no card with a body');
  return { ...made, boardId: card.boardId, cardId: card.id };
}

async function joined(body: BodyDoc): Promise<Y.Doc> {
  await vi.waitFor(() => {
    expect(body.state.status).toBe('joined');
  });
  const { doc } = body.state;
  if (!doc) throw new Error('joined without a document');
  return doc;
}

describe('the rooms of the backend in memory', () => {
  it('lets two readers edit one body together, and writes the card from it', async () => {
    const { world: stored, backend, other, boardId, cardId } = world();
    const changes: Change[] = [];
    backend.watchBoard(boardId, (change) => changes.push(change));
    const mine = backend.openDoc('w', boardId, cardId, listener());
    const theirs = other.openDoc('w', boardId, cardId, listener());
    const [a, b] = await Promise.all([joined(mine), joined(theirs)]);
    // Both hold the server's document of the stored body, made once.
    expect(bodyOf(a)).toEqual(stored.blocks.get(cardId)?.body);
    expect(bodyOf(b)).toEqual(bodyOf(a));

    append(a, 'from a');
    append(b, 'from b');
    await vi.waitFor(() => {
      expect(texts(a)).toEqual(texts(b));
      expect(texts(a).slice(-2).sort()).toEqual([
        '<paragraph>from a</paragraph>',
        '<paragraph>from b</paragraph>',
      ]);
      expect(mine.state.pending || theirs.state.pending).toBe(false);
    });

    roomsOf(stored).flush();
    const card = stored.blocks.get(cardId);
    expect(card?.body).toEqual(bodyOf(a));
    await vi.waitFor(() => {
      expect(changes.some((change) => change.kind === 'blocks')).toBe(true);
    });
    mine.close();
    theirs.close();
    expect(roomsOf(stored).members(cardId)).toBe(0);
  });

  it('lets a viewer in to read: they see the changes and cannot write', async () => {
    const { world: stored, backend, other, boardId, cardId } = world();
    await backend.setMemberRole(boardId, other.userId, 'viewer');
    const mine = backend.openDoc('w', boardId, cardId, listener());
    const told = listener();
    const theirs = other.openDoc('w', boardId, cardId, told);
    const [a, b] = await Promise.all([joined(mine), joined(theirs)]);
    expect(theirs.state).toMatchObject({ mode: 'read', writable: false });
    append(a, 'from a');
    await vi.waitFor(() => {
      expect(texts(b)).toEqual(texts(a));
    });
    // Written in a document read only (as an editor's own repair would): never sent.
    append(b, 'from b');
    const length = texts(a).length;
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(texts(a)).toHaveLength(length);
    roomsOf(stored).flush();
    expect(stored.blocks.get(cardId)?.body).toEqual(bodyOf(a));
    mine.close();
    theirs.close();
  });

  it('turns a reader whose role is lowered to reading, and puts out one taken off the board', async () => {
    const { backend, other, boardId, cardId } = world();
    const theirs = other.openDoc('w', boardId, cardId, listener());
    await joined(theirs);
    await backend.setMemberRole(boardId, other.userId, 'commenter');
    await vi.waitFor(() => {
      expect(theirs.state).toMatchObject({ mode: 'read', writable: false });
    });
    await backend.removeMember(boardId, other.userId);
    await vi.waitFor(() => {
      expect(theirs.state).toMatchObject({ status: 'out', reason: 'not_found', doc: undefined });
    });
    theirs.close();
  });

  it('refuses who is no member', async () => {
    const { backend, other, boardId, cardId } = world();
    await backend.removeMember(boardId, other.userId);
    const theirs = other.openDoc('w', boardId, cardId, listener());
    await vi.waitFor(() => {
      expect(theirs.state).toMatchObject({ status: 'out', reason: 'not_found', retrying: false });
    });
  });
});

class FakeSocket extends EventTarget {
  static made: FakeSocket[] = [];
  readonly texts: Record<string, unknown>[] = [];
  readonly frames: Uint8Array[] = [];
  binaryType = 'blob';
  readonly url: string;

  constructor(url: string) {
    super();
    this.url = url;
    FakeSocket.made.push(this);
  }

  send(data: string | Uint8Array): void {
    if (typeof data === 'string') this.texts.push(JSON.parse(data) as Record<string, unknown>);
    else this.frames.push(data);
  }

  close(): void {
    // The page closed it: no event, as the live connection no longer listens.
  }

  open(): void {
    this.dispatchEvent(new Event('open'));
  }

  message(data: unknown): void {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(data) }));
  }

  binary(bytes: Uint8Array): void {
    const buffer = bytes.slice().buffer;
    this.dispatchEvent(new MessageEvent('message', { data: buffer }));
  }

  drop(): void {
    this.dispatchEvent(new Event('close'));
  }
}

describe('a room on the page connection', () => {
  beforeEach(() => {
    FakeSocket.made = [];
    vi.stubGlobal('WebSocket', FakeSocket);
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  function server(): { live: LiveChanges; socket: () => FakeSocket } {
    const live = new LiveChanges('ws://test/ws', { members: () => Promise.resolve([]) });
    live.enter('w1');
    return {
      live,
      socket: () => {
        const socket = FakeSocket.made.at(-1);
        if (!socket) throw new Error('no socket');
        return socket;
      },
    };
  }

  /** The server's document of a body with one paragraph. */
  function whole(): Uint8Array {
    const doc = new Y.Doc();
    append(doc, 'stored');
    return Y.encodeStateAsUpdate(doc);
  }

  it('opens the connection for a room, joins, takes the whole, and sends a change as a frame', () => {
    const { live, socket } = server();
    const told = listener();
    const body = openRoom(live.rooms.line('w1', 'c1'), told);
    socket().open();
    expect(socket().texts).toContainEqual({ action: 'JOIN_DOC', workspaceId: 'w1', cardId: 'c1' });
    socket().message({
      action: 'DOC_JOINED',
      workspaceId: 'w1',
      cardId: 'c1',
      doc: 5,
      epoch: 2,
      mode: 'write',
    });
    // A message of the subscription comes between the join and the whole: no harm.
    socket().message({ action: 'UPDATE_BOARD', workspaceId: 'w1', board: { id: 'b' } });
    socket().binary(encodeDocFrame({ doc: 5, epoch: 2, sync: SYNC_STEP_2, payload: whole() }));
    expect(body.state.status).toBe('joined');
    const doc = body.state.doc;
    if (!doc) throw new Error('no document');
    expect(texts(doc)).toEqual(['<paragraph>stored</paragraph>']);

    append(doc, 'mine');
    expect(socket().frames).toHaveLength(0);
    vi.advanceTimersByTime(50);
    expect(socket().frames).toHaveLength(1);
    const sent = decodeDocFrame(socket().frames[0] ?? new Uint8Array());
    expect(sent).toMatchObject({ doc: 5, epoch: 2, sync: SYNC_UPDATE });
    expect(body.state.pending).toBe(true);
    socket().message({ action: 'DOC_ACK', doc: 5, epoch: 2, seq: 1 });
    expect(body.state.pending).toBe(false);

    // Another's change; and one of a room not this one, dropped.
    const theirs = new Y.Doc();
    Y.applyUpdate(theirs, Y.encodeStateAsUpdate(doc));
    const before = Y.encodeStateVector(theirs);
    append(theirs, 'theirs');
    const change = Y.encodeStateAsUpdate(theirs, before);
    socket().binary(encodeDocFrame({ doc: 9, epoch: 2, sync: SYNC_UPDATE, payload: change }));
    expect(texts(doc)).toHaveLength(2);
    socket().binary(encodeDocFrame({ doc: 5, epoch: 2, sync: SYNC_UPDATE, payload: change }));
    expect(texts(doc)).toHaveLength(3);

    body.close();
    expect(socket().texts.at(-1)).toEqual({ action: 'LEAVE_DOC', doc: 5 });
  });

  it('is read only while the connection is away, and joins again when it is back', () => {
    const { live, socket } = server();
    const stored = whole();
    const body = openRoom(live.rooms.line('w1', 'c1'), listener());
    socket().open();
    socket().message({
      action: 'DOC_JOINED',
      workspaceId: 'w1',
      cardId: 'c1',
      doc: 1,
      epoch: 2,
      mode: 'write',
    });
    socket().binary(encodeDocFrame({ doc: 1, epoch: 2, sync: SYNC_STEP_2, payload: stored }));
    const first = socket();
    first.drop();
    expect(body.state).toMatchObject({ status: 'offline', writable: false });
    vi.advanceTimersByTime(600);
    expect(socket()).not.toBe(first);
    socket().open();
    expect(socket().texts).toContainEqual({ action: 'JOIN_DOC', workspaceId: 'w1', cardId: 'c1' });
    expect(body.state.status).toBe('receiving');
    socket().message({
      action: 'DOC_JOINED',
      workspaceId: 'w1',
      cardId: 'c1',
      doc: 1,
      epoch: 2,
      mode: 'write',
    });
    socket().binary(encodeDocFrame({ doc: 1, epoch: 2, sync: SYNC_STEP_2, payload: stored }));
    expect(body.state).toMatchObject({ status: 'joined', writable: true });
    // The same epoch: the document held goes on, nothing doubled.
    expect(texts(body.state.doc)).toHaveLength(1);
    body.close();
  });

  it('gives each of two cards its answer, in the order they were asked', () => {
    const { live, socket } = server();
    const one = openRoom(live.rooms.line('w1', 'c1'), listener());
    const two = openRoom(live.rooms.line('w1', 'c2'), listener());
    socket().open();
    socket().message({
      action: 'DOC_REFUSED',
      workspaceId: 'w1',
      cardId: 'c2',
      reason: 'not_found',
    });
    socket().message({
      action: 'DOC_JOINED',
      workspaceId: 'w1',
      cardId: 'c1',
      doc: 3,
      epoch: 1,
      mode: 'read',
    });
    expect(two.state).toMatchObject({ status: 'out', reason: 'not_found' });
    expect(one.state.status).toBe('joining');
    one.close();
    two.close();
  });

  it('closes the connection once neither a room nor a listener needs it', () => {
    const { live, socket } = server();
    const body = openRoom(live.rooms.line('w1', 'c1'), listener());
    socket().open();
    const closing = vi.spyOn(socket(), 'close');
    body.close();
    expect(closing).toHaveBeenCalled();
  });
});
