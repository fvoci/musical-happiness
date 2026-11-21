/**
 * The workspaces of the world in memory, as the server has them: the one its
 * people share (`World.workspace`), and every user's own space, made the
 * first time it is asked for. The own space is a workspace like another to
 * what is under it, with its owner alone in it: of anybody else's it is not
 * there (404), and nothing of it is told to anybody else.
 */
import type { WorkspaceInfo } from '../workspace-types.ts';

import { BackendError } from '../problems.ts';

import { fail } from './refuse.ts';
import { copy } from './shapes.ts';
import type { World } from './world.ts';

/** The user's own space; made, with its default category, the first time it is asked for. */
export function ownSpaceOf(world: World, userId: string): WorkspaceInfo {
  const kept = world.ownSpaces.get(userId);
  if (kept) return kept;
  const own: WorkspaceInfo = { id: world.newId(), title: '', kind: 'personal' };
  world.ownSpaces.set(userId, own);
  const now = world.now();
  world.categories.set(userId, [
    ...(world.categories.get(userId) ?? []),
    {
      id: world.newId(),
      name: 'Boards',
      userId,
      workspaceId: own.id,
      type: 'system',
      collapsed: false,
      sortOrder: 0,
      containerIds: [],
      hiddenContainerIds: [],
      createAt: now,
      updateAt: now,
    },
  ]);
  return own;
}

/** The user's workspaces, as the server lists them: their own space last. */
export function workspacesOf(world: World, userId: string): WorkspaceInfo[] {
  return [copy(world.workspace), copy(ownSpaceOf(world, userId))];
}

/** Who works in the workspace: its owner alone in an own space; nobody in one that is not there. */
export function workersOf(world: World, workspaceId: string): ReadonlySet<string> {
  if (workspaceId === world.workspace.id) return world.workspaceMembers;
  for (const [owner, own] of world.ownSpaces) if (own.id === workspaceId) return new Set([owner]);
  return new Set();
}

/**
 * 404 unless the user works in the workspace: the shared one, or their own
 * space; 401 `password_sign_in_required` in an own space marked closed to the
 * session (`passwordRequired`), as the server answers a provider's session.
 */
export function checkWorkspace(world: World, userId: string, workspaceId: string): void {
  const own = ownSpaceOf(world, userId);
  if (workspaceId !== world.workspace.id && workspaceId !== own.id) {
    fail(404, 'no such workspace');
  }
  if (workspaceId === own.id && own.passwordRequired === true) {
    throw new BackendError(401, {
      reason: 'password_sign_in_required',
      detail: 'the own space opens to a session made with the password',
    });
  }
}
