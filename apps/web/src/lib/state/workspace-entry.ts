/**
 * Entering a workspace (`Workspace.enter`): which one to start in, and what
 * is read of it.
 */
import { createBoardView, type Container } from '../logic/wire.ts';

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

/** Open to this session: not of an organization that takes its single sign-on alone. */
export const isOpen = (workspace: WorkspaceInfo): boolean => !workspace.ssoRequired;

/**
 * The workspace to start in when the address names none: `last`, the one
 * the reader was last in on this device, if it is still theirs; else the
 * first open to this session that they work in, before one they are only a
 * guest of; else the first open to this session; else their first.
 */
export function firstChoiceOf(
  workspaces: readonly WorkspaceInfo[],
  last: string | undefined,
): string | undefined {
  const chosen =
    workspaces.find((workspace) => workspace.id === last) ??
    // Not one that is closed to this session while another is open: from a
    // closed one the reader has only the way through its single sign-on.
    workspaces.find((workspace) => isOpen(workspace) && workspace.role !== 'guest') ??
    workspaces.find(isOpen) ??
    workspaces[0];
  return chosen?.id;
}

/**
 * The boards and the wikis of a workspace the reader can see, the two lists
 * of the sidebar together (a wiki is a board of the kind `wiki`).
 */
export async function modulesOf(backend: Backend, workspaceId: string): Promise<Container[]> {
  const [boards, wikis] = await Promise.all([
    backend.boards(workspaceId),
    backend.wiki?.wikis(workspaceId) ?? [],
  ]);
  return [...boards, ...wikis];
}

/** The templates a board can be made from; none (told) when they cannot be read. */
export async function templatesOf(workspace: {
  readonly backend: Backend;
  readonly info: WorkspaceInfo | undefined;
  report(action: 'load', error: unknown): void;
}): Promise<Container[]> {
  if (!workspace.info) return [];
  try {
    return await workspace.backend.templates(workspace.info.id);
  } catch (error) {
    workspace.report('load', error);
    return [];
  }
}

/**
 * The people, boards and sidebar of a workspace. Undefined when the backend
 * says the reader has no such workspace (403, 404: left meanwhile, or never
 * theirs), as for one they do not work in.
 */
export async function readWorkspace(
  backend: Backend,
  workspaceId: string,
): Promise<{ users: User[]; containers: Container[]; categories: Category[] } | undefined> {
  try {
    const [users, containers, categories] = await Promise.all([
      backend.users(workspaceId),
      modulesOf(backend, workspaceId),
      backend.categories(workspaceId),
    ]);
    return { users, containers, categories };
  } catch (error) {
    if (isBackendError(error) && (error.status === 403 || error.status === 404)) return undefined;
    throw error;
  }
}

/**
 * What came of entering a workspace of the reader's: `info` is the workspace
 * entered, and `closed` the one that is theirs and closed to this session.
 */
export type Entered =
  | {
      status: 'ready';
      info: WorkspaceInfo;
      closed: undefined;
      users: User[];
      containers: Container[];
      categories: Category[];
    }
  | { status: 'missing'; info: undefined; closed: undefined }
  | { status: 'sso'; info: undefined; closed: WorkspaceInfo };

/**
 * Enters a workspace of the reader's: follows it and reads it. One that the
 * list says is closed to this session is not read, and one that turns out
 * closed while it is read (its organization began to take no password since
 * the list was read) is told the same: it is still theirs, known by its name,
 * with the organization to sign in to.
 */
export async function enterWorkspace(backend: Backend, workspace: WorkspaceInfo): Promise<Entered> {
  const none = { info: undefined, closed: undefined };
  if (workspace.ssoRequired && workspace.orgId)
    return { ...none, status: 'sso', closed: workspace };
  backend.enter?.(workspace.id);
  try {
    const read = await readWorkspace(backend, workspace.id);
    return read
      ? // Each entry its own: what was asked on an earlier visit is not taken for this one's.
        { status: 'ready', info: { ...workspace }, closed: undefined, ...read }
      : { ...none, status: 'missing' };
  } catch (error) {
    const orgId = ssoOrgOf(error);
    if (orgId === undefined) throw error;
    return { ...none, status: 'sso', closed: { ...workspace, orgId, ssoRequired: true } };
  }
}

/**
 * What a refusal that says an organization takes no password (401,
 * `sso_required`) makes of the reader's workspaces: those of that
 * organization are closed to this session, and `closed` is the one shown,
 * `shown`, if it is one of them (a refusal of something of another
 * organization, still on its way when the reader moved on, closes nothing on
 * the page). Undefined for any other error.
 */
export function closedBySso(
  workspaces: readonly WorkspaceInfo[],
  shown: WorkspaceInfo | undefined,
  error: unknown,
): { workspaces: WorkspaceInfo[]; closed: WorkspaceInfo | undefined } | undefined {
  const orgId = ssoOrgOf(error);
  if (orgId === undefined) return undefined;
  const ofIt = shown !== undefined && (shown.orgId === undefined || shown.orgId === orgId);
  return {
    workspaces: workspaces.map((workspace) =>
      workspace.orgId === orgId ? { ...workspace, ssoRequired: true } : workspace,
    ),
    closed: ofIt ? { ...shown, orgId, ssoRequired: true } : undefined,
  };
}

/**
 * The first view of a board that is made: a kanban grouped by the first
 * select property among `cardProperties`, showing all its options.
 */
export function firstKanban(
  cardProperties: Container['cardProperties'],
  title: string,
): { title: string; fields: ReturnType<typeof createBoardView>['fields'] } {
  const groupBy = cardProperties.find((template) => template.type === 'select');
  const fields = createBoardView().fields;
  fields.viewType = 'board';
  fields.groupById = groupBy?.id;
  fields.visibleOptionIds = [...(groupBy?.options.map((option) => option.id) ?? []), ''];
  return { title, fields };
}
