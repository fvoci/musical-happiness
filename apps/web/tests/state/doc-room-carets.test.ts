/**
 * The carets of a body opened in its room, between readers of the backend
 * in memory (`memory/room-carets.ts` through the room's client): each sees
 * where the others who write are, under the room's word of whose a caret
 * is; who reads shows none; a caret goes with its reader; and the tabs of a
 * browser tell each other's.
 */
import type { Awareness } from 'y-protocols/awareness';
import { describe, expect, it, vi } from 'vitest';
import * as Y from 'yjs';

import type { BodyDoc, BodyDocHears } from '$lib/state/backend.ts';
import { FIELD } from '$lib/state/memory/rooms.ts';

import { setup } from './support.ts';

const hears = (): BodyDocHears => ({ state: () => undefined, discarded: () => undefined });

function world(): ReturnType<typeof setup> & { boardId: string; cardId: string } {
  const made = setup();
  const card = [...made.world.blocks.values()].find((block) => block.type === 'card' && block.body);
  if (!card) throw new Error('the seed has no card with a body');
  return { ...made, boardId: card.boardId, cardId: card.id };
}

/** The carets of a body once it is joined. */
async function caretsOf(body: BodyDoc): Promise<{ awareness: Awareness; doc: Y.Doc }> {
  await vi.waitFor(() => {
    expect(body.state.status).toBe('joined');
  });
  const { carets, doc } = body.state;
  if (!carets || !doc) throw new Error('joined without a document and its carets');
  return { awareness: carets, doc };
}

/** Puts the reader's caret at the start of the body, as the editor's plugin does; the place as the room tells it. */
function show(held: { awareness: Awareness; doc: Y.Doc }, user: object = {}): object {
  const place = Y.createRelativePositionFromTypeIndex(held.doc.getXmlFragment(FIELD), 0);
  held.awareness.setLocalStateField('user', user);
  held.awareness.setLocalStateField('cursor', { anchor: place, head: place });
  // As the room tells it: what the place says, without what is null.
  const said = JSON.parse(JSON.stringify(place)) as Record<string, unknown>;
  return Object.fromEntries(Object.entries(said).filter(([, value]) => value !== null));
}

/** The states of the others a reader holds: all but their own. */
const others = (awareness: Awareness): Record<string, unknown>[] =>
  [...awareness.getStates()]
    .filter(([client]) => client !== awareness.clientID)
    .map(([, state]) => state);

describe('the carets of the rooms in memory', () => {
  it("shows each who writes where the other is, as the room's word of whose the caret is", async () => {
    const { backend, other, boardId, cardId } = world();
    const mine = backend.openDoc('w', boardId, cardId, hears());
    const theirs = other.openDoc('w', boardId, cardId, hears());
    const [a, b] = await Promise.all([caretsOf(mine), caretsOf(theirs)]);

    // As somebody else: nothing of it but the place is told.
    const place = show(a, { id: other.userId, name: 'Mallory', color: '#ff0000' });
    await vi.waitFor(() => {
      expect(others(b.awareness)).toEqual([
        { user: { id: backend.userId }, cursor: { anchor: place, head: place } },
      ]);
    });
    // Nobody is told their own caret.
    expect(others(a.awareness)).toEqual([]);
    show(b);
    await vi.waitFor(() => {
      expect(others(a.awareness)).toMatchObject([{ user: { id: other.userId } }]);
    });

    // The caret withdrawn (the editor lost the focus) is gone for the other.
    a.awareness.setLocalStateField('cursor', null);
    await vi.waitFor(() => {
      expect(others(b.awareness)).toEqual([]);
    });
    mine.close();
    theirs.close();
  });

  it('tells a reader the carets and shows none of theirs; a writer made a reader loses theirs', async () => {
    const { backend, other, boardId, cardId } = world();
    await backend.setMemberRole(boardId, other.userId, 'viewer');
    const mine = backend.openDoc('w', boardId, cardId, hears());
    const theirs = other.openDoc('w', boardId, cardId, hears());
    const [a, b] = await Promise.all([caretsOf(mine), caretsOf(theirs)]);
    expect(theirs.state.mode).toBe('read');
    show(a);
    show(b);
    await vi.waitFor(() => {
      expect(others(b.awareness)).toHaveLength(1);
    });
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(others(a.awareness)).toEqual([]);

    // Let write, the room is told the caret they hold; made a reader again, it is gone.
    await backend.setMemberRole(boardId, other.userId, 'editor');
    await vi.waitFor(() => {
      expect(others(a.awareness)).toMatchObject([{ user: { id: other.userId } }]);
    });
    await backend.setMemberRole(boardId, other.userId, 'viewer');
    await vi.waitFor(() => {
      expect(others(a.awareness)).toEqual([]);
    });
    mine.close();
    theirs.close();
  });

  it('takes a caret away with its reader, and tells who comes in the carets there are', async () => {
    const { backend, other, boardId, cardId } = world();
    const mine = backend.openDoc('w', boardId, cardId, hears());
    const theirs = other.openDoc('w', boardId, cardId, hears());
    const [a, b] = await Promise.all([caretsOf(mine), caretsOf(theirs)]);
    show(a);
    show(b);
    await vi.waitFor(() => {
      expect(others(a.awareness)).toHaveLength(1);
    });
    theirs.close();
    await vi.waitFor(() => {
      expect(others(a.awareness)).toEqual([]);
    });

    // Who comes in later is told the caret that is there.
    const late = other.openDoc('w', boardId, cardId, hears());
    const c = await caretsOf(late);
    await vi.waitFor(() => {
      expect(others(c.awareness)).toMatchObject([{ user: { id: backend.userId } }]);
    });
    // Taken off the board, their caret goes with them.
    show(c);
    await vi.waitFor(() => {
      expect(others(a.awareness)).toHaveLength(1);
    });
    await backend.removeMember(boardId, other.userId);
    await vi.waitFor(() => {
      expect(others(a.awareness)).toEqual([]);
    });
    mine.close();
    late.close();
  });

  it("tells the rooms of the browser's other tabs, each caret under a number of its own", async () => {
    const first = world();
    const second = world();
    const channel = `caret-test-${crypto.randomUUID()}`;
    first.world.shareRooms = channel;
    second.world.shareRooms = channel;
    const { boardId, cardId } = first;
    // The same person in two tabs: a caret each.
    const here = first.backend.openDoc('w', boardId, cardId, hears());
    const there = second.backend.openDoc('w', boardId, cardId, hears());
    const [a, b] = await Promise.all([caretsOf(here), caretsOf(there)]);
    show(a);
    show(b);
    await vi.waitFor(() => {
      expect(others(a.awareness)).toMatchObject([{ user: { id: second.backend.userId } }]);
      expect(others(b.awareness)).toMatchObject([{ user: { id: first.backend.userId } }]);
    });
    // A third reader, in the first tab's world, is told both.
    const third = first.other.openDoc('w', boardId, cardId, hears());
    const c = await caretsOf(third);
    await vi.waitFor(() => {
      expect(others(c.awareness)).toHaveLength(2);
    });
    const numbers = [...c.awareness.getStates().keys()].filter((n) => n !== c.awareness.clientID);
    expect(new Set(numbers).size).toBe(2);
    there.close();
    await vi.waitFor(() => {
      expect(others(a.awareness)).toEqual([]);
      expect(others(c.awareness)).toHaveLength(1);
    });
    here.close();
    third.close();
  });

  it('holds back what the room sends while the reader composes a syllable, for a few seconds at most', async () => {
    const { backend, other, boardId, cardId } = world();
    const mine = backend.openDoc('w', boardId, cardId, hears());
    const theirs = other.openDoc('w', boardId, cardId, hears());
    const [a, b] = await Promise.all([caretsOf(mine), caretsOf(theirs)]);
    const lines = (doc: Y.Doc): number => doc.getXmlFragment(FIELD).length;
    const write = (doc: Y.Doc, text: string): void => {
      const paragraph = new Y.XmlElement('paragraph');
      paragraph.insert(0, [new Y.XmlText(text)]);
      doc.getXmlFragment(FIELD).insert(lines(doc), [paragraph]);
    };
    const before = lines(a.doc);

    // Composing: the other's line and caret wait; the reader's own writing goes out all the same.
    mine.state.composing(true);
    write(b.doc, 'theirs');
    show(b);
    write(a.doc, 'mine');
    await vi.waitFor(() => {
      expect(lines(b.doc)).toBe(before + 2);
    });
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(lines(a.doc)).toBe(before + 1);
    expect(others(a.awareness)).toEqual([]);
    // The syllable is done: what waited is taken in, in its order.
    mine.state.composing(false);
    expect(lines(a.doc)).toBe(before + 2);
    expect(others(a.awareness)).toHaveLength(1);

    // A composition that does not end keeps the others' writing away three seconds at most.
    mine.state.composing(true);
    write(b.doc, 'later');
    await new Promise((resolve) => setTimeout(resolve, 1000));
    expect(lines(a.doc)).toBe(before + 2);
    await vi.waitFor(
      () => {
        expect(lines(a.doc)).toBe(before + 3);
      },
      { timeout: 4000 },
    );
    mine.close();
    theirs.close();
  }, 15_000);
});
