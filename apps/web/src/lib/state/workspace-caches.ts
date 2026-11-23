/**
 * Coordinates the three navigation caches owned by a `Workspace`: sidebar
 * snapshots (`WorkspaceCache`), recent board stores (`BoardCache`), and
 * last-opened wiki pages (`WikiCache`). Security and isolation invalidations
 * fan out to all three from one place.
 */
import type { Container } from '../logic/wire.ts';

import { ssoOrgOf, type Backend, type Category, type User, type WorkspaceInfo } from './backend.ts';
import { BoardCache, MAX_CACHED_BOARDS, type CacheableBoard } from './board-cache.ts';
import { MAX_CACHED_WIKIS, WikiCache } from './wiki-cache.ts';
import {
  MAX_CACHED_WORKSPACES,
  WorkspaceCache,
  mergeWorkspaceUsers,
  type WorkspaceSnapshot,
} from './workspace-cache.ts';
import * as entry from './workspace-entry.ts';
import { reread } from './write.ts';

export interface CacheLimits {
  workspaces?: number;
  boards?: number;
  wikis?: number;
}

export class WorkspaceCaches<B extends CacheableBoard = CacheableBoard> {
  readonly workspaces: WorkspaceCache;
  readonly boards: BoardCache<B>;
  readonly wikis: WikiCache;

  constructor(limits: CacheLimits = {}) {
    this.workspaces = new WorkspaceCache(limits.workspaces ?? MAX_CACHED_WORKSPACES);
    this.boards = new BoardCache<B>(limits.boards ?? MAX_CACHED_BOARDS);
    this.wikis = new WikiCache(limits.wikis ?? MAX_CACHED_WIKIS);
  }

  /** Immediately discards every cached workspace, board and wiki entry. */
  clear(): void {
    this.workspaces.clear();
    this.boards.clear();
    this.wikis.clear();
  }

  evictWorkspace(workspaceId: string, skipBoard?: B): void {
    this.workspaces.delete(workspaceId);
    this.boards.deleteWorkspace(workspaceId, skipBoard);
    this.wikis.deleteWorkspace(workspaceId);
  }

  evictOrg(workspaces: readonly WorkspaceInfo[], orgId: string, skipBoard?: B): void {
    this.workspaces.deleteOrg(workspaces, orgId);
    this.boards.deleteOrg(workspaces, orgId, skipBoard);
    this.wikis.deleteOrg(workspaces, orgId);
  }

  evictPersonal(workspaces: readonly WorkspaceInfo[], skipBoard?: B): void {
    this.workspaces.deletePersonal(workspaces);
    this.boards.deletePersonal(workspaces, skipBoard);
    this.wikis.deletePersonal(workspaces);
  }

  evictContainer(containerId: string, skipBoard?: B): void {
    this.boards.delete(containerId, skipBoard);
    this.wikis.delete(containerId);
  }

  pruneWorkspaces(workspaces: readonly WorkspaceInfo[], skipBoard?: B): void {
    this.workspaces.pruneWorkspaces(workspaces);
    this.boards.pruneWorkspaces(workspaces, skipBoard);
    this.wikis.pruneWorkspaces(workspaces);
  }

  pruneContainers(workspaceId: string, containers: readonly Container[], skipBoard?: B): void {
    const allowed = new Set(containers.map((container) => container.id));
    this.boards.pruneContainers(workspaceId, allowed, skipBoard);
    this.wikis.pruneContainers(workspaceId, allowed);
  }

  /** Evicts caches affected by an SSO or personal-space password lock error. */
  evictForClosing(
    workspaces: readonly WorkspaceInfo[],
    error: unknown,
    entered: 'sso' | 'password',
    closedId: string | undefined,
    skipBoard?: B,
  ): void {
    if (closedId !== undefined) this.evictWorkspace(closedId, skipBoard);
    if (entered === 'password') {
      this.evictPersonal(workspaces, skipBoard);
      return;
    }
    const orgId = ssoOrgOf(error);
    if (orgId !== undefined) this.evictOrg(workspaces, orgId, skipBoard);
  }

  /** Evicts caches when entering or revalidating a workspace ends in missing/sso/password. */
  evictForEntered(
    workspaces: readonly WorkspaceInfo[],
    workspaceId: string,
    read: Exclude<entry.Entered, { status: 'ready' }>,
    skipBoard?: B,
  ): void {
    if (read.status === 'missing') this.evictWorkspace(workspaceId, skipBoard);
    else if (read.status === 'password') this.evictPersonal(workspaces, skipBoard);
    else if (read.closed.orgId) this.evictOrg(workspaces, read.closed.orgId, skipBoard);
    else this.evictWorkspace(workspaceId, skipBoard);
  }
}

/** Host interface for background stale-while-revalidate of a cached workspace. */
export interface CachedEntryHost<B extends CacheableBoard> {
  readonly backend: Backend;
  readonly board: B | undefined;
  workspaces: WorkspaceInfo[];
  info: WorkspaceInfo | undefined;
  closed: WorkspaceInfo | undefined;
  entered: entry.EnterStatus;
  users: User[];
  workspaceUsers: User[];
  containers: Container[];
  categories: Category[];
  ends(error: unknown, reading?: boolean): boolean;
}

export interface CachedEntryHooks {
  isCurrent(turn: number, visit: WorkspaceInfo): boolean;
  sidebarBegun(): number;
  sidebarReading(begun: number): 'show' | 'later' | 'again';
  refreshSidebar(): void;
  rememberWorkspace(): void;
  leaveWithoutCaching(asked: string): void;
}

/**
 * Shows a cached workspace snapshot immediately, subscribes live updates, and
 * revalidates in the background (stale-while-revalidate). Access revocations
 * (401/403/404, SSO, password lock) evict caches and close the workspace at once.
 */
export async function enterCachedWorkspace<B extends CacheableBoard>(
  host: CachedEntryHost<B>,
  caches: WorkspaceCaches<B>,
  hooks: CachedEntryHooks,
  turn: number,
  workspace: WorkspaceInfo,
  cached: WorkspaceSnapshot,
): Promise<void> {
  const visit = { ...workspace };
  [host.info, host.closed, host.entered] = [visit, undefined, 'ready'];
  host.users = cached.users;
  host.workspaceUsers = cached.workspaceUsers;
  host.containers = cached.containers;
  host.categories = cached.categories;
  host.backend.enter?.(workspace.id);
  const before = host.containers.map((one) => one.id);
  const begun = hooks.sidebarBegun();
  try {
    const read = await entry.enterWorkspace(host.backend, visit, false);
    if (read.status !== 'ready') {
      caches.evictForEntered(host.workspaces, visit.id, read, host.board);
      if (!hooks.isCurrent(turn, visit)) return;
      host.workspaces = entry.withClosed(host.workspaces, read.closed);
      hooks.leaveWithoutCaching(visit.id);
      [host.closed, host.entered] = [read.closed, read.status];
      return;
    }
    if (!hooks.isCurrent(turn, visit)) return;
    host.containers = reread(host.containers, read.containers, before);
    const verdict = hooks.sidebarReading(begun);
    if (verdict === 'show') host.categories = read.categories;
    else if (verdict === 'again') hooks.refreshSidebar();
    host.users = mergeWorkspaceUsers(host.users, cached.workspaceUsers, read.users);
    host.workspaceUsers = read.users;
    hooks.rememberWorkspace();
    caches.pruneContainers(visit.id, host.containers, host.board);
  } catch (error) {
    // Background revalidation: end session or close workspace on 401; keep stale view on transient errors.
    host.ends(error, true);
  }
}
