/**
 * When the body of an open card is stored: after a pause in the typing, at
 * once when asked, and never for a report of the editor that changes
 * nothing. Runs on the card store and the in-memory backend.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { BodySaver, SAVE_PAUSE, sameBody } from '$lib/features/card/body-saver.svelte.ts';
import { BackendError } from '$lib/state/index.ts';

import { openFirstBoard, text } from '../../state/support.ts';

async function open(pause = 20) {
  const made = await openFirstBoard();
  const alpha = made.board.realCards.find((card) => card.title === 'Alpha');
  if (!alpha) throw new Error('no card');
  const card = await made.board.showCard(alpha.id);
  const saver = new BodySaver(card, pause);
  const stored = async () => (await made.backend.card(made.board.boardId, alpha.id)).body;
  return { ...made, card, saver, stored, cardId: alpha.id };
}

const wait = (milliseconds: number): Promise<void> =>
  new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });

afterEach(() => {
  vi.useRealTimers();
});

describe('two bodies that say the same', () => {
  it('are the same whatever order their members were written in', () => {
    const stored = { type: 'text', text: 'a', marks: [{ type: 'bold' }] };
    const edited = { marks: [{ type: 'bold' }], text: 'a', type: 'text' };
    expect(sameBody(stored, edited)).toBe(true);
  });

  it('differ when a text, a mark or the order of the nodes differs', () => {
    const first = { type: 'doc', content: [text('a'), text('b')] };
    expect(sameBody(first, { type: 'doc', content: [text('b'), text('a')] })).toBe(false);
    expect(sameBody(text('a'), text('b'))).toBe(false);
    expect(sameBody(text('a'), null)).toBe(false);
    expect(sameBody(null, null)).toBe(true);
  });

  it('take a member that is undefined for one that is not there', () => {
    expect(sameBody({ type: 'paragraph', attrs: undefined }, { type: 'paragraph' })).toBe(true);
  });
});

describe('the saving of a body', () => {
  it('waits for the typing to pause by default for 800 milliseconds', () => {
    expect(SAVE_PAUSE).toBe(800);
  });

  it('starts idle', async () => {
    const { saver } = await open();
    expect(saver.state).toBe('idle');
  });

  it('stores an edit once the typing pauses, and says so', async () => {
    const { saver, stored } = await open();
    saver.edited(text('first'));
    expect(saver.state).toBe('saving');
    expect(await stored()).toEqual(text('first body'));
    await wait(60);
    expect(saver.state).toBe('saved');
    expect(await stored()).toEqual(text('first'));
  });

  it('starts the pause again with every edit, and stores the newest', async () => {
    const { saver, stored, backend } = await open(40);
    const writes = vi.spyOn(backend, 'writeCardBody');
    saver.edited(text('o'));
    await wait(15);
    saver.edited(text('on'));
    await wait(15);
    saver.edited(text('one'));
    await wait(15);
    expect(writes).not.toHaveBeenCalled();
    await wait(80);
    expect(writes).toHaveBeenCalledTimes(1);
    expect(await stored()).toEqual(text('one'));
  });

  it('stores at once when asked: the editor was left, the card is closing', async () => {
    const { saver, stored } = await open(10_000);
    saver.edited(text('leaving'));
    await saver.flush();
    expect(await stored()).toEqual(text('leaving'));
    expect(saver.state).toBe('saved');
  });

  it('sends nothing when asked to store without an edit', async () => {
    const { saver, backend } = await open();
    const writes = vi.spyOn(backend, 'writeCardBody');
    await saver.flush();
    expect(writes).not.toHaveBeenCalled();
    expect(saver.state).toBe('idle');
  });

  it('takes a report of the body it already has for no edit at all', async () => {
    const { saver, card, backend } = await open();
    const writes = vi.spyOn(backend, 'writeCardBody');
    // The stored body with its members in another order, as an editor writes them.
    saver.edited({
      content: [{ content: [{ text: 'first body', type: 'text' }], type: 'paragraph' }],
      type: 'doc',
    });
    expect(card.dirty).toBe(false);
    expect(saver.state).toBe('idle');
    await wait(60);
    expect(writes).not.toHaveBeenCalled();
  });

  it('takes a second report of the same edit for nothing new', async () => {
    const { saver, backend } = await open();
    const writes = vi.spyOn(backend, 'writeCardBody');
    saver.edited(text('once'));
    await saver.flush();
    saver.edited(text('once'));
    expect(saver.state).toBe('saved');
    await wait(60);
    expect(writes).toHaveBeenCalledTimes(1);
  });

  it("compares with the stored body again after somebody else's body was taken", async () => {
    const { saver, card, other, board, cardId, backend } = await open();
    saver.edited(text('mine'));
    await saver.flush();
    const theirs = await other.card(board.boardId, cardId);
    await other.writeCardBody(board.boardId, cardId, {
      body: text('theirs'),
      readAt: theirs.updateAt,
    });
    await wait(30);
    expect(card.body).toEqual(text('theirs'));
    const writes = vi.spyOn(backend, 'writeCardBody');
    // The editor took their body and reports it: not an edit.
    saver.edited(text('theirs'));
    expect(card.dirty).toBe(false);
    // What the reader had before is an edit again.
    saver.edited(text('mine'));
    await saver.flush();
    expect(writes).toHaveBeenCalledTimes(1);
  });

  it('says that the body is not saved when the write fails, and sends it with the next one', async () => {
    const { saver, backend, stored } = await open();
    backend.failNext('writeCardBody', new BackendError(500));
    saver.edited(text('kept'));
    await saver.flush();
    expect(saver.state).toBe('unsaved');
    await saver.flush();
    expect(saver.state).toBe('saved');
    expect(await stored()).toEqual(text('kept'));
  });

  async function conflict() {
    const made = await open(10_000);
    const { saver, other, board, cardId } = made;
    saver.edited(text('mine'));
    const theirs = await other.card(board.boardId, cardId);
    await other.writeCardBody(board.boardId, cardId, {
      body: text('theirs'),
      readAt: theirs.updateAt,
    });
    await saver.flush();
    expect(saver.state).toBe('conflict');
    return made;
  }

  it('holds a conflict until the reader chooses, and stores nothing meanwhile', async () => {
    const { saver, stored } = await conflict();
    await saver.flush();
    expect(saver.state).toBe('conflict');
    expect(await stored()).toEqual(text('theirs'));
  });

  it("keeps the reader's text on their word, as the editor has it now", async () => {
    const { saver, card, stored } = await conflict();
    // Typed between the refused write and the notice.
    saver.edited(text('mine, and more'));
    await saver.keepMine();
    expect(card.conflict).toBeUndefined();
    expect(saver.state).toBe('saved');
    expect(await stored()).toEqual(text('mine, and more'));
  });

  it('takes the stored text on their word, and drops what they wrote', async () => {
    const { saver, card, stored } = await conflict();
    const revision = card.bodyRevision;
    await saver.takeTheirs();
    expect(card.conflict).toBeUndefined();
    expect(card.body).toEqual(text('theirs'));
    expect(card.bodyRevision).toBeGreaterThan(revision);
    expect(card.dirty).toBe(false);
    expect(await stored()).toEqual(text('theirs'));
    // The editor reports the text it was given: nothing to store.
    saver.edited(text('theirs'));
    expect(card.dirty).toBe(false);
  });

  it('stops its clock when it is disposed of', async () => {
    const { saver, backend } = await open(20);
    const writes = vi.spyOn(backend, 'writeCardBody');
    saver.edited(text('never sent by the clock'));
    saver.dispose();
    await wait(60);
    expect(writes).not.toHaveBeenCalled();
  });
});
