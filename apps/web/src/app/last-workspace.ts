/**
 * The workspace each user was last in on this browser, so that the root
 * address takes them back to it. Kept per user: someone else signing in
 * here is not taken to (nor told of) a workspace of the one before.
 *
 * Only the identifier is kept. Whether it is still theirs is the backend's
 * say (`Workspace.load` enters it only when it is among their workspaces).
 */
const KEY = 'fvoci:last-workspace:';

export function lastWorkspace(userId: string): string | undefined {
  try {
    return localStorage.getItem(`${KEY}${userId}`) ?? undefined;
  } catch {
    // No storage here (blocked, or a private window): the first workspace then.
    return undefined;
  }
}

export function rememberWorkspace(userId: string, workspaceId: string): void {
  try {
    localStorage.setItem(`${KEY}${userId}`, workspaceId);
  } catch {
    // Without storage the choice lasts for the visit.
  }
}

const PLACE_KEY = 'fvoci:last-place:';

/**
 * The board or the wiki each user was last on in a workspace on this
 * browser: the workspace's address alone takes them back to it, if the
 * sidebar still has it (`WorkspacePane.svelte`).
 */
export function lastPlace(userId: string, workspaceId: string): string | undefined {
  try {
    return localStorage.getItem(`${PLACE_KEY}${userId}:${workspaceId}`) ?? undefined;
  } catch {
    // No storage here: the first of the sidebar then.
    return undefined;
  }
}

export function rememberPlace(userId: string, workspaceId: string, boardId: string): void {
  try {
    localStorage.setItem(`${PLACE_KEY}${userId}:${workspaceId}`, boardId);
  } catch {
    // Without storage the place lasts for the visit.
  }
}
