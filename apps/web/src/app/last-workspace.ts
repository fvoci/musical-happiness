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
