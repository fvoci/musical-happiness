/**
 * The content of a board for the invited only, in the memory backend as on
 * the server: a reader who is not among its members is told it is not there
 * (404), while the board and its members stay readable for those who manage
 * it (`src/lib/state/managed-board.ts`). The e2e tests and the Storybook's
 * managed board run on this backend; the story wraps it to refuse by hand.
 */
import { describe, expect, it } from 'vitest';

import { BackendError } from '$lib/state/index.ts';
import { MemoryBackend, memberOf } from '$lib/state/memory/backend.ts';
import { World } from '$lib/state/memory/world.ts';
import { createBoard, createBoardView, createCard, type Block } from '$lib/logic/wire.ts';

const NOW = Date.UTC(2026, 10, 13, 3);

function inviteOnlyBoard(): MemoryBackend {
  const world = new World(() => NOW);
  world.workspace = { id: 'w', title: 'Example', role: 'admin' };
  const board = {
    ...createBoard(),
    id: 'b',
    workspaceId: 'w',
    type: 'P' as const,
    title: 'Hidden',
  };
  world.boards.set(board.id, board);
  world.members.set(board.id, [memberOf(board.id, 'someone-else', 'admin')]);
  const block = (id: string, type: Block['type'], title: string): Block => ({
    id,
    type,
    boardId: 'b',
    parentId: 'b',
    createdBy: 'someone-else',
    modifiedBy: 'someone-else',
    schema: 1,
    title,
    fields: {},
    createAt: NOW,
    updateAt: NOW,
    deleteAt: 0,
  });
  const view = createBoardView(block('v', 'view', 'View'));
  const card = createCard(block('c', 'card', 'Secret title'));
  world.blocks.set(view.id, view);
  world.blocks.set(card.id, card);
  return new MemoryBackend(world, 'reader');
}

const statusOf = async (promise: Promise<unknown>): Promise<number | 'ok'> => {
  try {
    await promise;
    return 'ok';
  } catch (error) {
    if (error instanceof BackendError) return error.status;
    throw error;
  }
};

describe('a board the reader is not a member of', () => {
  it('does not give its views, cards or a card to the reader', async () => {
    const backend = inviteOnlyBoard();
    expect({
      views: await statusOf(backend.views('b')),
      cards: await statusOf(backend.cards('b')),
      card: await statusOf(backend.card('b', 'c')),
    }).toEqual({ views: 404, cards: 404, card: 404 });
  });

  it('still lets the reader see the board and its members, to manage them', async () => {
    const backend = inviteOnlyBoard();
    expect(await statusOf(backend.board('b'))).toBe('ok');
    expect(await statusOf(backend.members('b'))).toBe('ok');
  });
});
