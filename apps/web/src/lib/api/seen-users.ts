/**
 * The people of a workspace by their identifiers, as many as the reader sees.
 * `getWorkspaceUsersById` answers 404 for the whole list when one of them is
 * nobody the reader sees (somebody who left the workspace, a member of a board
 * without a role there): the list is halved until the ones the reader sees are
 * found, and the others are left out, unknown as the server has them.
 */
import { isBackendError } from '../state/problems.ts';

export async function seenUsers<T>(
  ids: readonly string[],
  ask: (ids: string[]) => Promise<T[]>,
): Promise<T[]> {
  if (ids.length === 0) return [];
  try {
    return await ask([...ids]);
  } catch (error) {
    if (!isBackendError(error) || error.status !== 404) throw error;
    if (ids.length === 1) return [];
    const half = Math.ceil(ids.length / 2);
    const parts = await Promise.all([
      seenUsers(ids.slice(0, half), ask),
      seenUsers(ids.slice(half), ask),
    ]);
    return parts.flat();
  }
}
