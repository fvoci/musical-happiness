/**
 * Card relations in the in-memory backend, as the server has them: a write
 * is refused for what the server refuses, by the same reason, and a copy of
 * a board names its own cards.
 */
import { describe, expect, it } from 'vitest';

import { BackendError } from '$lib/state/index.ts';
import { RELATION_LIMIT } from '$lib/logic/wire.ts';

import { setup } from './support.ts';

const RELATION = 'after';

async function relatedBoard() {
  const made = setup();
  const { backend } = made;
  const [workspace] = await backend.workspaces();
  const workspaceId = workspace?.id ?? '';
  const [board] = await backend.boards(workspaceId);
  if (!board) throw new Error('no board');
  await backend.patchContainer(board.id, {
    updatedCardProperties: [{ id: RELATION, name: 'After', type: 'relation', options: [] }],
  });
  const card = async (title: string) => (await backend.createCard(board.id, { title })).id;
  return { ...made, workspaceId, boardId: board.id, card };
}

const reasonOf = async (promise: Promise<unknown>): Promise<string | undefined> => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof BackendError) {
      expect(error.status).toBe(400);
      return error.reason;
    }
    throw error;
  }
  return undefined;
};

describe('a relation value written', () => {
  it('is refused, by the reason the server gives, for what the server refuses', async () => {
    const { backend, boardId, card, workspaceId } = await relatedBoard();
    const [first, second] = [await card('First'), await card('Second')];
    const write = (value: unknown) =>
      backend.patchCard(boardId, first, { updatedProperties: { [RELATION]: value as string[] } });
    const many = Array.from({ length: RELATION_LIMIT + 1 }, (_, at) => `c${String(at)}`);
    // A card of another board of the same world: one of a copy of this board.
    const copy = await backend.duplicateContainer(boardId, false, workspaceId);
    const [elsewhere] = await backend.cards(copy.id);

    expect(await reasonOf(write(second))).toBe('relation_not_a_list');
    expect(await reasonOf(write([first]))).toBe('relation_to_itself');
    expect(await reasonOf(write([second, second]))).toBe('relation_repeated');
    expect(await reasonOf(write(many))).toBe('relation_too_many');
    expect(await reasonOf(write(['no-such-card']))).toBe('relation_card_not_found');
    expect(await reasonOf(write([elsewhere?.id ?? '']))).toBe('relation_card_not_found');
    const [view] = await backend.views(boardId);
    expect(await reasonOf(write([view?.id ?? '']))).toBe('relation_not_a_card');
    expect(await reasonOf(write([second]))).toBeUndefined();
    const stored = (await backend.card(boardId, first)).fields.properties[RELATION];
    expect(stored).toEqual([second]);
  });

  it('may name a card in the trash, and keeps naming it', async () => {
    const { backend, boardId, card } = await relatedBoard();
    const [first, deleted, third] = [await card('First'), await card('Gone'), await card('Third')];
    await backend.deleteBlock(boardId, deleted);
    await backend.patchCard(boardId, first, { updatedProperties: { [RELATION]: [deleted] } });
    await backend.patchCard(boardId, first, {
      updatedProperties: { [RELATION]: [deleted, third] },
    });
    expect((await backend.card(boardId, first)).fields.properties[RELATION]).toEqual([
      deleted,
      third,
    ]);
  });
});

describe('a copy of a board', () => {
  it('names the copies of the cards, and drops what it cannot name so', async () => {
    const { backend, boardId, card, workspaceId } = await relatedBoard();
    const [first, second, deleted] = [
      await card('First'),
      await card('Second'),
      await card('Gone'),
    ];
    await backend.patchCard(boardId, second, {
      updatedProperties: { [RELATION]: [first, deleted] },
    });
    await backend.deleteBlock(boardId, deleted);

    const copy = await backend.duplicateContainer(boardId, false, workspaceId);
    const cards = await backend.cards(copy.id);
    const byTitle = (title: string) => cards.find((each) => each.title === title);
    expect(byTitle('Second')?.fields.properties[RELATION]).toEqual([byTitle('First')?.id]);
  });
});

describe('a relation value stored before the rules', () => {
  /** A card whose value names itself, a card twice, and a card that never was, as an older server kept it. */
  async function stale() {
    const made = await relatedBoard();
    const { world, card } = made;
    const [first, second] = [await card('First'), await card('Second')];
    const many = Array.from({ length: RELATION_LIMIT }, (_, at) => `old-${String(at)}`);
    const stored = world.blocks.get(first);
    if (!stored) throw new Error('no card');
    stored.fields = {
      ...stored.fields,
      properties: { [RELATION]: [first, 'never-was', 'never-was', ...many], note: 'before' },
    };
    return { ...made, first, second, many };
  }

  it('does not stop a write that leaves it alone', async () => {
    const { backend, boardId, first } = await stale();
    const patched = await backend.patchCard(boardId, first, {
      title: 'Renamed',
      updatedProperties: { note: 'after' },
    });
    expect(patched.fields.properties.note).toBe('after');
  });

  it('may lose cards over the limit, and not ask again for what it named', async () => {
    const { backend, boardId, first, second, many } = await stale();
    const write = (value: string[]) =>
      backend.patchCard(boardId, first, { updatedProperties: { [RELATION]: value } });
    const error = await write(['never-was', ...many, second]).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(BackendError);
    expect(error).toMatchObject({
      reason: 'relation_too_many',
      limit: RELATION_LIMIT,
      errors: [{ pointer: '/updatedProperties', reason: 'relation_too_many' }],
    });
    expect(await reasonOf(write(['never-was', ...many]))).toBeUndefined();
    expect(await reasonOf(write(['never-was', second]))).toBeUndefined();
    expect(await reasonOf(write(['never-was', second, 'not-either']))).toBe(
      'relation_card_not_found',
    );
  });

  it('is made one a write takes in a copy of the board', async () => {
    const { backend, boardId, workspaceId } = await stale();
    const copy = await backend.duplicateContainer(boardId, false, workspaceId);
    const cards = await backend.cards(copy.id);
    expect(cards.find((each) => each.title === 'First')?.fields.properties[RELATION]).toEqual([]);
  });
});

describe('a new card', () => {
  it('is refused at the member its values are in', async () => {
    const { backend, boardId } = await relatedBoard();
    const error = await backend
      .createCard(boardId, { title: 'New', properties: { [RELATION]: ['nope'] } })
      .catch((caught: unknown) => caught);
    expect(error).toMatchObject({
      status: 400,
      reason: 'relation_card_not_found',
      type: '/problems/relation-card-not-found',
      errors: [{ pointer: '/properties', reason: 'relation_card_not_found' }],
    });
  });
});
