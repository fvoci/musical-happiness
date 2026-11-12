/**
 * Entering a workspace (`Workspace.enter`): which one to start in, and what
 * is read of it.
 */
import type { Board } from '../logic/wire.ts';

import {
  isBackendError,
  ssoOrgOf,
  type Backend,
  type Category,
  type User,
  type WorkspaceInfo,
} from './backend.ts';

/**
 * How the workspace asked for stands: `none` when none was (the reader works
 * in none), `missing` when the reader works in no workspace by that
 * identifier (there is none, or it is not theirs: the two are not told apart),
 * `sso` when it is theirs and closed to this session: its organization takes
 * no password, and it opens for a session made through its single sign-on.
 */
export type EnterStatus = 'none' | 'loading' | 'ready' | 'missing' | 'failed' | 'sso';

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

/** What came of entering a workspace of the reader's, and the workspace as it is shown then. */
export type Entered =
  | { status: 'ready'; info: WorkspaceInfo; users: User[]; boards: Board[]; categories: Category[] }
  | { status: 'missing'; info: undefined }
  | { status: 'sso'; info: WorkspaceInfo };

/**
 * Enters a workspace of the reader's: follows it and reads it. One that the
 * list says is closed to this session is not read, and one that turns out
 * closed while it is read (its organization began to take no password since
 * the list was read) is told the same: it is still theirs, shown by its name,
 * with the organization to sign in to.
 */
export async function enterWorkspace(backend: Backend, workspace: WorkspaceInfo): Promise<Entered> {
  if (workspace.ssoRequired && workspace.orgId) return { status: 'sso', info: workspace };
  backend.enter?.(workspace.id);
  try {
    const read = await readWorkspace(backend, workspace.id);
    return read
      ? { status: 'ready', info: workspace, ...read }
      : { status: 'missing', info: undefined };
  } catch (error) {
    const orgId = ssoOrgOf(error);
    if (orgId === undefined) throw error;
    return { status: 'sso', info: { ...workspace, orgId, ssoRequired: true } };
  }
}

/**
 * What a refusal that says an organization takes no password (401, `sso_required`)
 * makes of the reader's workspaces and of the one shown, `shown`: those of
 * that organization are closed to this session. Undefined for any other
 * error, and when no workspace is shown.
 */
export function closedBySso(
  workspaces: readonly WorkspaceInfo[],
  shown: WorkspaceInfo | undefined,
  error: unknown,
): { workspaces: WorkspaceInfo[]; info: WorkspaceInfo } | undefined {
  const orgId = ssoOrgOf(error);
  if (orgId === undefined || !shown) return undefined;
  return {
    workspaces: workspaces.map((workspace) =>
      workspace.orgId === orgId ? { ...workspace, ssoRequired: true } : workspace,
    ),
    info: { ...shown, orgId, ssoRequired: true },
  };
}
