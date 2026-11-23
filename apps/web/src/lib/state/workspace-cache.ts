/**
 * Bounded LRU cache of per-workspace sidebar snapshots (`users`, `containers`,
 * `categories`), scoped to one signed-in user. Returning to a visited workspace
 * shows its snapshot at once while fresh data is read in the background
 * (stale-while-revalidate).
 */
import type { Container } from '../logic/wire.ts';

import { isOwnSpace, type Category, type User, type WorkspaceInfo } from './backend.ts';

/** How many workspaces' sidebar snapshots stay in memory at once. */
export const MAX_CACHED_WORKSPACES = 5;

export interface WorkspaceSnapshot {
  readonly userId: string;
  readonly workspaceId: string;
  readonly users: User[];
  readonly workspaceUsers: User[];
  readonly containers: Container[];
  readonly categories: Category[];
}

export class WorkspaceCache {
  readonly #max: number;
  readonly #entries = new Map<string, WorkspaceSnapshot>();
  #ownerId: string | undefined;

  constructor(max = MAX_CACHED_WORKSPACES) {
    this.#max = Math.max(1, max);
  }

  get size(): number {
    return this.#entries.size;
  }

  has(workspaceId: string): boolean {
    return this.#entries.has(workspaceId);
  }

  /**
   * Returns the cached snapshot for `(userId, workspaceId)` and promotes it to
   * most-recently-used. If the cache holds data for another user, everything is
   * cleared first so nothing ever leaks across accounts.
   */
  get(userId: string, workspaceId: string): WorkspaceSnapshot | undefined {
    if (this.#ownerId !== undefined && this.#ownerId !== userId) {
      this.clear();
      return undefined;
    }
    const entry = this.#entries.get(workspaceId);
    if (!entry) return undefined;
    if (entry.userId !== userId) {
      this.clear();
      return undefined;
    }
    this.#entries.delete(workspaceId);
    this.#entries.set(workspaceId, entry);
    return entry;
  }

  /** Stores or updates a snapshot, evicting the least-recently-used over the cap. */
  set(snapshot: WorkspaceSnapshot): void {
    if (this.#ownerId !== undefined && this.#ownerId !== snapshot.userId) {
      this.clear();
    }
    this.#ownerId = snapshot.userId;
    this.#entries.delete(snapshot.workspaceId);
    this.#entries.set(snapshot.workspaceId, snapshot);
    while (this.#entries.size > this.#max) {
      const oldest = this.#entries.keys().next().value;
      if (oldest === undefined) break;
      this.#entries.delete(oldest);
    }
  }

  delete(workspaceId: string): void {
    this.#entries.delete(workspaceId);
  }

  deleteOrg(workspaces: readonly WorkspaceInfo[], orgId: string): void {
    for (const workspace of workspaces) {
      if (workspace.orgId === orgId) this.#entries.delete(workspace.id);
    }
  }

  deletePersonal(workspaces: readonly WorkspaceInfo[]): void {
    for (const workspace of workspaces) {
      if (isOwnSpace(workspace)) this.#entries.delete(workspace.id);
    }
  }

  /** Removes entries for workspaces no longer open to the reader. */
  pruneWorkspaces(workspaces: readonly WorkspaceInfo[]): void {
    const open = new Set(
      workspaces
        .filter((workspace) => !workspace.ssoRequired && !workspace.passwordRequired)
        .map((workspace) => workspace.id),
    );
    for (const id of [...this.#entries.keys()]) {
      if (!open.has(id)) this.#entries.delete(id);
    }
  }

  clear(): void {
    this.#entries.clear();
    this.#ownerId = undefined;
  }
}

/**
 * Merges a freshly read workspace user list into `current`: fresh members replace
 * older copies, people removed from `cachedWorkspaceUsers` are dropped, and
 * extra users learned during the visit (such as card authors) are kept.
 */
export function mergeWorkspaceUsers(
  current: readonly User[],
  cachedWorkspaceUsers: readonly User[],
  freshWorkspaceUsers: readonly User[],
): User[] {
  const freshIds = new Set(freshWorkspaceUsers.map((user) => user.id));
  const oldWorkspaceIds = new Set(cachedWorkspaceUsers.map((user) => user.id));
  const extras = current.filter((user) => !freshIds.has(user.id) && !oldWorkspaceIds.has(user.id));
  return [...freshWorkspaceUsers, ...extras];
}
