/** User names are shared by the screens of one workspace visit. */
import type { DocNode } from '../editor/schema.ts';
import { isGone, userDisplayName } from '../logic/users.ts';

import { takeUser } from './account.ts';
import { isBackendError, type User, type WorkspaceInfo } from './backend.ts';
import type { Workspace } from './workspace.svelte.ts';

/**
 * Whom the blocks name: who made and last changed each, and whom the body of
 * each mentions. Somebody mentioned may be no member any more, or be gone:
 * their name is asked for like a writer's.
 */
export function authorIds(
  blocks: readonly { createdBy: string; modifiedBy: string; body?: DocNode | null }[],
): string[] {
  return blocks.flatMap((block) => [block.createdBy, block.modifiedBy, ...mentionedIn(block.body)]);
}

/** The users a body mentions, each as often as it does. */
export function mentionedIn(body: DocNode | null | undefined): string[] {
  if (!body) return [];
  const userId: unknown = body.type === 'mention' ? body.attrs?.userId : undefined;
  const below = (body.content ?? []).flatMap(mentionedIn);
  return typeof userId === 'string' && userId !== '' ? [userId, ...below] : below;
}

/**
 * Whom the cards name in their properties of people (`person`, `multiPerson`):
 * somebody a card is set to may be no member of the board any more, or be gone.
 */
export function assignedIds(
  cards: readonly { fields: { properties: Record<string, unknown> } }[],
  properties: readonly { id: string; type: string }[],
): string[] {
  const ofPeople = properties.filter(({ type }) => type === 'person' || type === 'multiPerson');
  return cards.flatMap((card) =>
    ofPeople.flatMap(({ id }) => {
      const value = card.fields.properties[id];
      const ids: unknown[] = Array.isArray(value) ? value : [value];
      return ids.filter((one): one is string => typeof one === 'string' && one !== '');
    }),
  );
}

export const memberId = (member: { userId: string }): string => member.userId;

export interface Learning {
  /**
   * Who are members now (`true`: all of them; else those listed): one left out earlier in the
   * visit is asked again.
   */
  members?: true | readonly string[];
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

  readonly #goneName: (() => string) | undefined;

  /**
   * `goneName` names somebody whose account was deleted, in the app's
   * language. Without it they are as unknown as somebody without a name here.
   */
  constructor(workspace: Workspace, goneName?: () => string) {
    this.#workspace = workspace;
    this.#goneName = goneName;
  }

  /**
   * The name to show for a user: "Display name (@login)". Somebody whose
   * account was deleted has none left: what they wrote stays under an
   * identifier, and they are named as the app names a deleted user.
   */
  nameOf(user: User): string | undefined {
    return isGone(user) ? this.#goneName?.() : userDisplayName(user);
  }

  /**
   * People of the workspace whose name has `query` in it (ten at most). A failure
   * finds nobody; a session that is over signs out.
   */
  async search(query: string): Promise<User[]> {
    const workspace = this.#workspace;
    if (!workspace.info) return [];
    try {
      return await workspace.backend.searchUsers(workspace.info.id, query);
    } catch (error) {
      if (isBackendError(error) && error.status === 401) workspace.report('load', error);
      else if (!isBackendError(error)) console.error(error);
      return [];
    }
  }

  async learn(userIds: readonly string[], learning: Learning = {}): Promise<void> {
    const workspace = this.#workspace;
    const visit = workspace.info;
    if (!visit) return;
    const pending = this.#pending.get(visit) ?? new Map<string, Promise<void>>();
    this.#pending.set(visit, pending);
    const answered = this.#answered.get(visit) ?? new Set<string>();
    this.#answered.set(visit, answered);
    const members = learning.members === true ? userIds : (learning.members ?? []);
    for (const id of members) answered.delete(id);
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

  /**
   * Reads again somebody whose name changed (a `user` change), as any reading
   * of them: whom the reader sees is the backend's rule. Somebody it leaves
   * out stays as they were.
   */
  async reread(userId: string): Promise<void> {
    const workspace = this.#workspace;
    const visit = workspace.info;
    if (!visit) return;
    try {
      const found = await workspace.backend.usersById(visit.id, [userId]);
      const user = found.find((one) => one.id === userId);
      if (user && workspace.info === visit) takeUser(workspace, user);
    } catch {
      // The next change, or the next reading of the workspace, brings them.
    }
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
