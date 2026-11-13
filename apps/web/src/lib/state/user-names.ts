/** User names are shared by the screens of one workspace visit. */
import { isBackendError, type User, type WorkspaceInfo } from './backend.ts';
import type { Workspace } from './workspace.svelte.ts';

export function authorIds(blocks: readonly { createdBy: string; modifiedBy: string }[]): string[] {
  return blocks.flatMap((block) => [block.createdBy, block.modifiedBy]);
}

/** Shares pending lookups and respects the server's limit of 200 identifiers per request. */
export class UserNames {
  readonly #workspace: Workspace;
  readonly #pending = new WeakMap<WorkspaceInfo, Map<string, Promise<void>>>();

  constructor(workspace: Workspace) {
    this.#workspace = workspace;
  }

  async learn(userIds: readonly string[]): Promise<void> {
    const workspace = this.#workspace;
    const visit = workspace.info;
    if (!visit) return;
    const pending = this.#pending.get(visit) ?? new Map<string, Promise<void>>();
    this.#pending.set(visit, pending);
    const missing = [...new Set(userIds)].filter((id) => id !== '' && !workspace.user(id));
    const fresh = missing.filter((id) => !pending.has(id));
    for (let start = 0; start < fresh.length; start += 200) {
      const ids = fresh.slice(start, start + 200);
      const reading = this.#read(visit, ids).finally(() => {
        for (const id of ids) pending.delete(id);
      });
      for (const id of ids) pending.set(id, reading);
    }
    await Promise.all(missing.flatMap((id) => pending.get(id) ?? []));
  }

  async #read(visit: WorkspaceInfo, ids: string[]): Promise<void> {
    const workspace = this.#workspace;
    try {
      const found = await workspace.backend.usersById(visit.id, ids);
      if (workspace.info !== visit) return;
      const added: User[] = found.filter(
        (user) => ids.includes(user.id) && !workspace.user(user.id),
      );
      workspace.users = [...workspace.users, ...added];
    } catch (error) {
      if (workspace.info !== visit) return;
      if (isBackendError(error) && error.status === 401) workspace.report('load', error);
      else if (!isBackendError(error)) console.error(error);
    }
  }
}
