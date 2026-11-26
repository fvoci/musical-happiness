/** Whether the reader watches a card (`BlockWatch`), on the backend in memory. */
import { describe, expect, it } from 'vitest';

import { BackendError, BlockWatch } from '$lib/state/index.ts';

import { openFirstBoard, settle } from './support.ts';

describe('watching a card', () => {
  it('reads that the writer of a card watches it, and stops and starts again', async () => {
    const { workspace, board, backend } = await openFirstBoard();
    const card = await backend.createCard(board.boardId, { title: 'Mine' });
    const watch = new BlockWatch(workspace, card.id, 'card');
    expect(watch.canWatch).toBe(true);
    expect(watch.watching).toBeUndefined();
    await watch.read();
    expect(watch.watching).toBe(true);

    await watch.set(false);
    expect(watch.watching).toBe(false);
    const meId = workspace.me?.id ?? '';
    const workspaceId = workspace.info?.id ?? '';
    expect(await backend.watches.watching(workspaceId, meId)).not.toContain(card.id);
    await watch.set(true);
    expect(await backend.watches.watching(workspaceId, meId)).toContain(card.id);
    watch.close();
  });

  it('goes back and reports when the server refuses', async () => {
    const { workspace, board, backend } = await openFirstBoard();
    const card = await backend.createCard(board.boardId, { title: 'Mine' });
    const watch = new BlockWatch(workspace, card.id, 'card');
    await watch.read();
    backend.failNext('watches', new BackendError(500, { detail: 'down' }));
    await watch.set(false);
    await settle();
    expect(watch.watching).toBe(true);
    expect(workspace.problems).toHaveLength(1);
  });
});
