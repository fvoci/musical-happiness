/**
 * Entering a workspace (`Workspace.enter`): which one to start in, and what
 * is read of it.
 */
import { createBoardView, type Container } from '../logic/wire.ts';

import {
  isBackendError,
  isOwnSpace,
  isPasswordSignInRequired,
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
 * no password, and it opens for a session made through its single sign-on;
 * `password` when it is their own space, closed to a session made through a
 * provider once the account has a password, and opens for one made with it.
 */
export type EnterStatus = 'none' | 'loading' | 'ready' | 'missing' | 'failed' | 'sso' | 'password';

/**
 * Open to this session: not of an organization that takes its single sign-on
 * alone, nor an own space that takes the password alone.
 */
export const isOpen = (workspace: WorkspaceInfo): boolean =>
  !workspace.ssoRequired && !workspace.passwordRequired;

/**
 * The workspace to start in when the address names none: `last`, the one
 * the reader was last in on this device, if it is still theirs; else one
 * they share with others, as a new account and one invited as a guest alone
 * start there: the first open to this session that they work in, before one
 * they are only a guest of; else the first open to this session; else the
 * first. Their own space only when they share none.
 */
export function firstChoiceOf(
  workspaces: readonly WorkspaceInfo[],
  last: string | undefined,
): string | undefined {
  const shared = workspaces.filter((workspace) => !isOwnSpace(workspace));
  const chosen =
    workspaces.find((workspace) => workspace.id === last) ??
    // Not one that is closed to this session while another is open: from a
    // closed one the reader has only the way through its single sign-on.
    shared.find((workspace) => isOpen(workspace) && workspace.role !== 'guest') ??
    shared.find(isOpen) ??
    shared[0] ??
    workspaces[0];
  return chosen?.id;
}

/**
 * Starts an organization by `start` (`Organizations.createOrganization`) and puts its
 * workspace among the reader's, which are read again in the backend's order;
 * the workspace made, to go to. Where the list cannot be read, the workspace
 * is put among those the page has, as it was answered.
 */
export async function startOrganization(
  workspace: { workspaces: WorkspaceInfo[]; readonly backend: Backend },
  start: (name: string) => Promise<WorkspaceInfo>,
  name: string,
): Promise<WorkspaceInfo> {
  const made = await start(name);
  try {
    workspace.workspaces = await workspace.backend.workspaces();
  } catch {
    workspace.workspaces = [...workspace.workspaces.filter((one) => one.id !== made.id), made];
  }
  return made;
}

/**
 * Takes the reader out of the organization `orgId` by `leave`
 * (`Account.leaveOrganization`), and reads their workspaces again, which have
 * none of it any more. Where the list cannot be read, the organization's are
 * taken out of those the page has. A refusal changes nothing, and rejects; so
 * does a 404, for a reader who is out of it already (left in another tab),
 * whose workspaces are read again as well.
 */
export async function leaveOrganization(
  workspace: { workspaces: WorkspaceInfo[]; readonly backend: Backend },
  leave: (orgId: string) => Promise<void>,
  orgId: string,
): Promise<void> {
  try {
    await leave(orgId);
  } catch (error) {
    if (isBackendError(error) && error.status === 404) await rereadWithout(workspace, orgId);
    throw error;
  }
  await rereadWithout(workspace, orgId);
}

/** Reads the reader's workspaces again, or takes out the organization's where they cannot be. */
async function rereadWithout(
  workspace: { workspaces: WorkspaceInfo[]; readonly backend: Backend },
  orgId: string,
): Promise<void> {
  try {
    workspace.workspaces = await workspace.backend.workspaces();
  } catch {
    workspace.workspaces = workspace.workspaces.filter((one) => one.orgId !== orgId);
  }
}

/**
 * The boards, the wikis and the diaries of a workspace the reader can see,
 * the lists of the sidebar together (each is a board of its `kind`). Only the
 * reader's own space has diaries: they are not asked for of another, which
 * the server answers as one that is not there.
 */
export async function modulesOf(backend: Backend, workspace: WorkspaceInfo): Promise<Container[]> {
  const [boards, wikis, diaries, courses] = await Promise.all([
    backend.boards(workspace.id),
    backend.wiki?.wikis(workspace.id) ?? [],
    (isOwnSpace(workspace) && backend.diary?.diaries(workspace.id)) || [],
    backend.course?.courses(workspace.id) ?? [],
  ]);
  return [...boards, ...wikis, ...diaries, ...courses];
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
  workspace: WorkspaceInfo,
): Promise<{ users: User[]; containers: Container[]; categories: Category[] } | undefined> {
  try {
    const [users, containers, categories] = await Promise.all([
      backend.users(workspace.id),
      modulesOf(backend, workspace),
      backend.categories(workspace.id),
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
  | { status: 'sso' | 'password'; info: undefined; closed: WorkspaceInfo };

/**
 * Enters a workspace of the reader's: follows it and reads it. One that the
 * list says is closed to this session is not read, and one that turns out
 * closed while it is read (its organization began to take no password, or the
 * account was given one, since the list was read) is told the same: it is
 * still theirs, known by its name, with the way to sign in that opens it.
 */
export async function enterWorkspace(backend: Backend, workspace: WorkspaceInfo): Promise<Entered> {
  const none = { info: undefined, closed: undefined };
  if (workspace.ssoRequired && workspace.orgId)
    return { ...none, status: 'sso', closed: workspace };
  if (workspace.passwordRequired) return { ...none, status: 'password', closed: workspace };
  backend.enter?.(workspace.id);
  try {
    const read = await readWorkspace(backend, workspace);
    return read
      ? // Each entry its own: what was asked on an earlier visit is not taken for this one's.
        { status: 'ready', info: { ...workspace }, closed: undefined, ...read }
      : { ...none, status: 'missing' };
  } catch (error) {
    const closing = closedBy([workspace], workspace, error);
    if (!closing?.closed) throw error;
    return { ...none, status: closing.entered, closed: closing.closed };
  }
}

/**
 * The reader's workspaces with `closed`, the one entered and found closed to
 * this session, as it was found: a list read before it closed says so too.
 * The same list when none was.
 */
export function withClosed(
  workspaces: WorkspaceInfo[],
  closed: WorkspaceInfo | undefined,
): WorkspaceInfo[] {
  if (!closed) return workspaces;
  return workspaces.map((one) => (one.id === closed.id ? closed : one));
}

/**
 * What a refusal that closes workspaces to this session makes of the
 * reader's: an organization that takes no password (401, `sso_required`)
 * closes its workspaces, an own space that takes the password alone (401,
 * `password_sign_in_required`) closes the own space. `closed` is the one
 * shown, `shown`, if it is one of them (a refusal of something elsewhere,
 * still on its way when the reader moved on, closes nothing on the page), and
 * `entered` says how. Undefined for any other error.
 */
export function closedBy(
  workspaces: readonly WorkspaceInfo[],
  shown: WorkspaceInfo | undefined,
  error: unknown,
):
  | { workspaces: WorkspaceInfo[]; closed: WorkspaceInfo | undefined; entered: 'sso' | 'password' }
  | undefined {
  if (isPasswordSignInRequired(error)) {
    return {
      workspaces: workspaces.map((workspace) =>
        isOwnSpace(workspace) ? { ...workspace, passwordRequired: true } : workspace,
      ),
      closed: shown && isOwnSpace(shown) ? { ...shown, passwordRequired: true } : undefined,
      entered: 'password',
    };
  }
  const orgId = ssoOrgOf(error);
  if (orgId === undefined) return undefined;
  const ofIt = shown !== undefined && (shown.orgId === undefined || shown.orgId === orgId);
  return {
    workspaces: workspaces.map((workspace) =>
      workspace.orgId === orgId ? { ...workspace, ssoRequired: true } : workspace,
    ),
    closed: ofIt ? { ...shown, orgId, ssoRequired: true } : undefined,
    entered: 'sso',
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
