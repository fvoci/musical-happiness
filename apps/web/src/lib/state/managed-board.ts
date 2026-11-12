/**
 * A board for the invited only that the reader manages without reading it:
 * an administrator of the workspace sees it in the list, manages its members
 * and deletes it, and reads what is on it only as its member, which the
 * workspace's members then read in its records. Its content is not there for
 * the reader (404) while the board itself and its members are.
 */
import { isBackendError } from './backend.ts';
import { boardOfWorkspace } from './board-parts.ts';
import type { BoardStore } from './board.svelte.ts';

/**
 * Whether the board the store failed to open as missing is one the reader
 * manages without reading. Then the store is given the board and its members,
 * for the members to be managed and the reader to join it; it stays failed.
 */
export async function readManagedOnly(store: BoardStore): Promise<boolean> {
  const backend = store.workspace.backend;
  try {
    const [board, members] = await Promise.all([
      boardOfWorkspace(backend, store.boardId, store.workspace.info?.id),
      backend.members(store.boardId),
    ]);
    if (board.type !== 'P') return false;
    store.board = board;
    store.members = members;
    return true;
  } catch (error) {
    if (isBackendError(error) && (error.status === 404 || error.status === 403)) return false;
    throw error;
  }
}

/** Makes the reader a viewer of the board they manage, and opens it. False when that failed. */
export async function joinManaged(store: BoardStore): Promise<boolean> {
  const me = store.workspace.me;
  if (!me) return false;
  try {
    await store.workspace.backend.addMember(store.boardId, me.id, 'viewer');
  } catch (error) {
    store.workspace.report('save', error);
    return false;
  }
  await store.load();
  return store.status === 'ready';
}
