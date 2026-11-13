/** User names are shared by the screens of one workspace visit. */
import { isBackendError, type User, type WorkspaceInfo } from './backend.ts';
import type { Workspace } from './workspace.svelte.ts';

export function authorIds(blocks: readonly { createdBy: string; modifiedBy: string }[]): string[] {
  return blocks.flatMap((block) => [block.createdBy, block.modifiedBy]);
}

export const memberId = (member: { userId: string }): string => member.userId;

export interface Learning {
  /** The people are members now: one left out earlier in the visit is asked again. */
  members?: boolean;
}

/**
 * Shares pending lookups and respects the server's limit of 200 identifiers per request. An
 * identifier is asked once a visit: one the server leaves out (an account that was closed, a
 * writer the reader may not see) is not asked again until the next visit, or until it is
 * learned as a member's (let in since); a failure is asked again.
 */
export class UserNames {
  readonly #workspace: Workspace;
  readonly #pending = new WeakMap<WorkspaceInfo, Map<string, Promise<void>>>();
  readonly #answered = new WeakMap<WorkspaceInfo, Set<string>>();

  constructor(workspace: Workspace) {
    this.#workspace = workspace;
  }

  async learn(userIds: readonly string[], learning: Learning = {}): Promise<void> {
    const workspace = this.#workspace;
    const visit = workspace.info;
    if (!visit) return;
    const pending = this.#pending.get(visit) ?? new Map<string, Promise<void>>();
    this.#pending.set(visit, pending);
    const answered = this.#answered.get(visit) ?? new Set<string>();
    this.#answered.set(visit, answered);
    if (learning.members) for (const id of userIds) answered.delete(id);
    const missing = [...new Set(userIds)].filter(
      (id) => id !== '' && !answered.has(id) && !workspace.user(id),
    );
    const fresh = missing.filter((id) => !pending.has(id));
    for (let start = 0; start < fresh.length; start += 200) {
      const ids = fresh.slice(start, start + 200);
      const reading = this.#read(visit, ids, answered).finally(() => {
        for (const id of ids) pending.delete(id);
      });
      for (const id of ids) pending.set(id, reading);
    }
    await Promise.all(missing.flatMap((id) => pending.get(id) ?? []));
  }

  async #read(visit: WorkspaceInfo, ids: string[], answered: Set<string>): Promise<void> {
    const workspace = this.#workspace;
    try {
      const found = await workspace.backend.usersById(visit.id, ids);
      for (const id of ids) answered.add(id);
      if (workspace.info !== visit) return;
      const added: User[] = found.filter(
        (user) => ids.includes(user.id) && !workspace.user(user.id),
      );
      if (added.length > 0) workspace.users = [...workspace.users, ...added];
    } catch (error) {
      // A session that is over signs out, whichever workspace is shown by then.
      if (isBackendError(error) && error.status === 401) workspace.report('load', error);
      else if (!isBackendError(error)) console.error(error);
    }
  }
}
