/**
 * Entering a workspace (`Workspace.enter`): which one to start in, and what
 * is read of it.
 */
import type { Board } from '../logic/wire.ts';

import {
  isBackendError,
  type Backend,
  type Category,
  type User,
  type WorkspaceInfo,
} from './backend.ts';

/**
 * How the workspace asked for stands: `none` when none was (the reader works
 * in none), `missing` when the reader works in no workspace by that
 * identifier (there is none, or it is not theirs: the two are not told apart).
 */
export type EnterStatus = 'none' | 'loading' | 'ready' | 'missing' | 'failed';

/**
 * The workspace to start in when the address names none: `last`, the one
 * the reader was last in on this device, if it is still theirs, else their first.
 */
export function firstChoiceOf(
  workspaces: readonly WorkspaceInfo[],
  last: string | undefined,
): string | undefined {
  return (workspaces.find((workspace) => workspace.id === last) ?? workspaces[0])?.id;
}

/**
 * The people, boards and sidebar of a workspace. Undefined when the backend
 * says the reader has no such workspace (403, 404: left meanwhile, or never
 * theirs), as for one they do not work in.
 */
export async function readWorkspace(
  backend: Backend,
  workspaceId: string,
): Promise<{ users: User[]; boards: Board[]; categories: Category[] } | undefined> {
  try {
    const [users, boards, categories] = await Promise.all([
      backend.users(workspaceId),
      backend.boards(workspaceId),
      backend.categories(workspaceId),
    ]);
    return { users, boards, categories };
  } catch (error) {
    if (isBackendError(error) && (error.status === 403 || error.status === 404)) return undefined;
    throw error;
  }
}
