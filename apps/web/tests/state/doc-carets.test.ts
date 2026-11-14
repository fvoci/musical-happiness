/**
 * The carets of a body as the room's client holds them (`state/doc-carets.ts`):
 * when the reader's own is told to the room, and what is kept of the others'.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as Y from 'yjs';

import { caretTold } from '$lib/state/memory/caret-frame.ts';
import { CARET_WAIT, Carets } from '$lib/state/doc-carets.ts';

const CURSOR = { anchor: { tname: 'default', assoc: 0 }, head: { tname: 'default', assoc: 0 } };

/** Carets of a new document, and what they told the room. */
function carets(takes = true): { held: Carets; told: Uint8Array[]; doc: Y.Doc } {
  const doc = new Y.Doc();
  const told: Uint8Array[] = [];
  const held = new Carets(doc, (update) => {
    if (takes) told.push(update);
    return takes;
  });
  return { held, told, doc };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("the reader's caret", () => {
  it('is told once for what was set together, and no more often than every wait', () => {
    const { held, told } = carets();
    held.awareness.setLocalStateField('user', {});
    vi.advanceTimersByTime(CARET_WAIT);
    // No caret yet, and the room knows of none.
    expect(told).toHaveLength(0);
    held.awareness.setLocalStateField('cursor', CURSOR);
    held.awareness.setLocalStateField('cursor', { ...CURSOR, head: { tname: 'x', assoc: 0 } });
    vi.advanceTimersByTime(0);
    expect(told).toHaveLength(1);
    for (let moved = 0; moved < 5; moved += 1) {
      held.awareness.setLocalStateField('cursor', { ...CURSOR, head: { tname: `m${moved}` } });
    }
    vi.advanceTimersByTime(CARET_WAIT - 1);
    expect(told).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(told).toHaveLength(2);
    held.destroy();
  });

  it('is said again when the library renews it, and told gone once', () => {
    const { held, told } = carets();
    held.awareness.setLocalStateField('cursor', CURSOR);
    vi.advanceTimersByTime(CARET_WAIT);
    expect(told).toHaveLength(1);
    // The library renews a state that stays, by setting it again as it is (its clock is the
    // machine's, which the timers of a test do not move).
    held.awareness.setLocalState(held.awareness.getLocalState());
    vi.advanceTimersByTime(CARET_WAIT);
    expect(told).toHaveLength(2);
    held.awareness.setLocalStateField('cursor', null);
    vi.advanceTimersByTime(CARET_WAIT);
    expect(told).toHaveLength(3);
    // Gone, the renewals say nothing more.
    held.awareness.setLocalState(held.awareness.getLocalState());
    vi.advanceTimersByTime(CARET_WAIT);
    expect(told).toHaveLength(3);
    held.destroy();
  });

  it('is told anew once the room can be told again', () => {
    const { held, told } = carets();
    held.awareness.setLocalStateField('cursor', CURSOR);
    vi.advanceTimersByTime(CARET_WAIT);
    held.again();
    vi.advanceTimersByTime(CARET_WAIT);
    expect(told).toHaveLength(2);
    // A room that could not be told is told when it can be.
    const away = carets(false);
    away.held.awareness.setLocalStateField('cursor', CURSOR);
    vi.advanceTimersByTime(CARET_WAIT);
    expect(away.told).toHaveLength(0);
    away.held.destroy();
    held.destroy();
  });

  it('says nothing more once it is let go, and leaves no timer', () => {
    const { held, told } = carets();
    held.awareness.setLocalStateField('cursor', CURSOR);
    held.destroy();
    vi.advanceTimersByTime(60_000);
    expect(told).toHaveLength(0);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("the others' carets", () => {
  it('are taken in as the room tells them, and let go with which telling was the last when the room is left', () => {
    const { held } = carets();
    held.take(caretTold(41, 5, '{"user":{"id":"u"},"cursor":null}'));
    expect(held.awareness.getStates().get(41)).toEqual({ user: { id: 'u' }, cursor: null });
    // Bytes that are no update change nothing.
    held.take(new Uint8Array([9, 9, 9]));
    held.away();
    expect([...held.awareness.getStates().keys()]).toEqual([held.awareness.clientID]);
    // Another room, or another server, counts from one again: its word is taken.
    held.take(caretTold(41, 1, '{"user":{"id":"u"},"cursor":null}'));
    expect(held.awareness.getStates().has(41)).toBe(true);
    held.destroy();
  });

  it('are let go by themselves thirty seconds after the last word of them', () => {
    const { held } = carets();
    held.take(caretTold(41, 1, '{"user":{"id":"u"},"cursor":null}'));
    vi.advanceTimersByTime(4000);
    expect(held.awareness.getStates().has(41)).toBe(true);
    // The library looks every three seconds, by the machine's clock: as if the word were old.
    const heard = held.awareness.meta.get(41);
    if (heard) heard.lastUpdated -= 30_000;
    vi.advanceTimersByTime(4000);
    expect(held.awareness.getStates().has(41)).toBe(false);
    held.destroy();
  });
});
