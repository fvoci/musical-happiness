/**
 * Cards that repeat and the activity of cards in memory, as the server keeps
 * them: completing a card that repeats makes its next one, once; a board
 * without a rule of done and a due date refuses a recurrence; a card's lines
 * say what was done to it, one person's changes of one thing within five
 * minutes on one line.
 */
import { describe, expect, it } from 'vitest';

import { isBackendError } from '$lib/state/backend.ts';

import { setup, STATUS } from './support.ts';

const DUE = 'due';
const weekly = { every: 1, unit: 'week', anchor: 'due' } as const;

/** The small world, its board given a due date and `done` as its rule of done when `rules`. */
async function chores(rules = true) {
  const made = setup();
  const board = [...made.world.containers.values()].find((c) => c.title === 'Small');
  if (!board) throw new Error('the seed has no such board');
  await made.backend.patchContainer(board.id, {
    updatedCardProperties: [{ id: DUE, name: 'Due', type: 'date', options: [] }],
    ...(rules ? { doneWhen: { kind: 'option', propertyId: STATUS, optionIds: ['done'] } } : {}),
  });
  let now = Date.UTC(2027, 0, 15, 12);
  made.world.useClock(() => now);
  const later = (minutes: number): void => {
    now += minutes * 60_000;
  };
  return { ...made, boardId: board.id, later };
}

describe('a card that repeats, in memory', () => {
  it('makes its next card when it is done, and only once', async () => {
    const { backend, boardId } = await chores();
    const card = await backend.createCard(boardId, {
      title: 'Water',
      properties: { [STATUS]: 'doing', [DUE]: { from: '2027-01-15' } },
      recurrence: weekly,
      body: {
        type: 'doc',
        content: [
          {
            type: 'taskList',
            content: [
              {
                type: 'taskItem',
                attrs: { checked: true },
                content: [{ type: 'paragraph', content: [{ type: 'text', text: 'the fern' }] }],
              },
            ],
          },
        ],
      },
    });
    const done = await backend.patchCard(boardId, card.id, {
      updatedProperties: { [STATUS]: 'done' },
    });
    const nextId = done.fields.recurredTo;
    expect(nextId).toBeDefined();
    expect(done.fields.recurrence).toBeUndefined();
    const next = await backend.card(boardId, nextId ?? '');
    expect(next.title).toBe('Water');
    expect(next.number).toBe(card.number === undefined ? undefined : card.number + 1);
    expect(next.fields.recurrence).toEqual(weekly);
    expect(next.fields.properties).toEqual({ [STATUS]: 'todo', [DUE]: { from: '2027-01-22' } });
    expect(next.body?.content?.[0]?.content?.[0]?.attrs?.checked).toBe(false);

    await backend.patchCard(boardId, card.id, { updatedProperties: { [STATUS]: 'todo' } });
    const again = await backend.patchCard(boardId, card.id, {
      updatedProperties: { [STATUS]: 'done' },
    });
    expect(again.fields.recurredTo).toBe(nextId);
    expect((await backend.cards(boardId)).filter((c) => c.title === 'Water')).toHaveLength(2);
  });

  it('repeats anew when it is set again, and never on a board without the rules', async () => {
    const { backend, boardId } = await chores(false);
    const refused = await backend
      .createCard(boardId, { title: 'Sweep', recurrence: weekly })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.reason).toBe('recurrence_needs_board_rules');
    const card = await backend.createCard(boardId, { title: 'Sweep' });
    await expect(backend.patchCard(boardId, card.id, { recurrence: weekly })).rejects.toThrow();

    const ruled = await chores();
    const chore = await ruled.backend.createCard(ruled.boardId, {
      title: 'Dust',
      properties: { [STATUS]: 'todo' },
      recurrence: weekly,
    });
    const done = await ruled.backend.patchCard(ruled.boardId, chore.id, {
      updatedProperties: { [STATUS]: 'done' },
    });
    const undone = await ruled.backend.patchCard(ruled.boardId, chore.id, {
      updatedProperties: { [STATUS]: 'todo' },
      recurrence: weekly,
    });
    expect(done.fields.recurredTo).toBeDefined();
    expect(undone.fields.recurredTo).toBeUndefined();
    expect(undone.fields.recurrence).toEqual(weekly);
  });

  it('makes its next card when it is moved to the column of done', async () => {
    const { backend, boardId, world } = await chores();
    const card = await backend.createCard(boardId, {
      title: 'Mop',
      properties: { [STATUS]: 'todo' },
      recurrence: weekly,
    });
    const view = [...world.blocks.values()].find((block) => block.type === 'view');
    const moved = await backend.moveViewCards(boardId, view?.id ?? '', {
      cardIds: [card.id],
      groupPropertyId: STATUS,
      groupValue: 'done',
    });
    expect(moved.cards[0]?.fields.recurredTo).toBeDefined();
    expect((await backend.cards(boardId)).filter((c) => c.title === 'Mop')).toHaveLength(2);
  });
});

describe('the activity of a card, in memory', () => {
  it('keeps one line for one person’s changes of one thing within five minutes', async () => {
    const { backend, other, boardId, later } = await chores();
    const card = await backend.createCard(boardId, { title: 'first' });
    later(1);
    await backend.patchCard(boardId, card.id, { title: 'second' });
    later(2);
    await backend.patchCard(boardId, card.id, { title: 'third' });
    const lines = await backend.cardActivity.list(boardId, card.id);
    expect(lines.map((line) => [line.kind, line.before, line.after])).toEqual([
      ['title', 'first', 'third'],
      ['created', null, null],
    ]);
    // Back to what it was, the line goes; another's change is a line of its own.
    later(1);
    await backend.patchCard(boardId, card.id, { title: 'first' });
    await other.patchCard(boardId, card.id, { updatedProperties: { [STATUS]: 'done' } });
    later(10);
    await backend.patchCard(boardId, card.id, { title: 'fourth' });
    const now = await backend.cardActivity.list(boardId, card.id, { page: 0, perPage: 2 });
    expect(now.map((line) => [line.kind, line.after])).toEqual([
      ['title', 'fourth'],
      ['property', 'done'],
    ]);
    expect(now[1]?.actorId).not.toBe(now[0]?.actorId);
  });

  it('says a card came back, and is not read of a card in the trash', async () => {
    const { backend, boardId } = await chores();
    const card = await backend.createCard(boardId, { title: 'gone' });
    await backend.batchDeleteCards(boardId, [card.id]);
    await expect(backend.cardActivity.list(boardId, card.id)).rejects.toThrow();
    await backend.batchRestoreCards(boardId, [card.id]);
    const lines = await backend.cardActivity.list(boardId, card.id);
    expect(lines.map((line) => line.kind)).toEqual(['restored', 'created']);
  });
});
