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
  await backend.patchBoard(board.id, {
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
    const copy = await backend.duplicateBoard(boardId, false, workspaceId);
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

    const copy = await backend.duplicateBoard(boardId, false, workspaceId);
    const cards = await backend.cards(copy.id);
    const byTitle = (title: string) => cards.find((each) => each.title === title);
    expect(byTitle('Second')?.fields.properties[RELATION]).toEqual([byTitle('First')?.id]);
  });
});
