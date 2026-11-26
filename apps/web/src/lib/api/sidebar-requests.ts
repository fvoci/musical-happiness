/**
 * Sidebar category HTTP operations for `ServerBackend` (`api/backend.ts`),
 * serialized per workspace via `BlockTurns` and reusing the cached user ID (§1.3.5).
 */
import {
  BackendError,
  type Category,
  type CategoryDraft,
  type CategoryPatch,
} from '../state/backend.ts';
import type { BlockTurns } from '../state/write.ts';

import { categoryOf, type Schemas } from './convert.ts';
import type { Http } from './http.ts';
import { underWorkspace } from './requests.ts';

export class ServerSidebar {
  readonly #http: Http;
  readonly #turns: BlockTurns;
  readonly #userId: () => Promise<string>;

  constructor(http: Http, turns: BlockTurns, userId: () => Promise<string>) {
    this.#http = http;
    this.#turns = turns;
    this.#userId = userId;
  }

  async categories(workspaceId: string): Promise<Category[]> {
    const categories = await this.#http.request<Schemas['CategoryContainers'][]>(
      underWorkspace('/categories'),
      { workspaceId },
    );
    return categories.map(categoryOf);
  }

  #turn<T>(workspaceId: string, write: () => Promise<T>): Promise<T> {
    return this.#turns.run(`sidebar ${workspaceId}`, write);
  }

  createCategory(workspaceId: string, draft: CategoryDraft): Promise<Category> {
    return this.#turn(workspaceId, async () => {
      const userId = await this.#userId();
      const request: Schemas['NewCategory'] = { name: draft.name, userId, workspaceId };
      const made = await this.#http.request<Schemas['Category']>(
        underWorkspace('/categories'),
        { workspaceId },
        { method: 'POST', json: request },
      );
      return categoryOf({ ...made, containerMetadata: [] });
    });
  }

  patchCategory(workspaceId: string, categoryId: string, patch: CategoryPatch): Promise<Category> {
    return this.#turn(workspaceId, async () => {
      const before = await this.#category(workspaceId, categoryId);
      const request: Schemas['CategoryUpdate'] = {
        id: categoryId,
        userId: before.userId,
        workspaceId,
        name: patch.name ?? before.name,
        collapsed: patch.collapsed ?? before.collapsed,
      };
      const updated = await this.#http.request<Schemas['Category']>(
        underWorkspace('/categories/{categoryId}'),
        { workspaceId, categoryId },
        { method: 'PUT', json: request },
      );
      return {
        ...categoryOf({ ...updated, containerMetadata: [] }),
        containerIds: before.containerIds,
        hiddenContainerIds: before.hiddenContainerIds,
      };
    });
  }

  deleteCategory(workspaceId: string, categoryId: string): Promise<void> {
    return this.#turn(workspaceId, async () => {
      await this.#http.request(
        underWorkspace('/categories/{categoryId}'),
        { workspaceId, categoryId },
        { method: 'DELETE' },
      );
    });
  }

  reorderCategories(workspaceId: string, categoryIds: string[]): Promise<void> {
    return this.#turn(workspaceId, async () => {
      await this.#http.request(
        underWorkspace('/category-order'),
        { workspaceId },
        { method: 'PUT', json: categoryIds },
      );
    });
  }

  moveContainerToCategory(
    workspaceId: string,
    containerId: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
    return this.#turn(workspaceId, async () => {
      await this.#http.request(
        underWorkspace('/categories/{categoryId}/containers/{containerId}'),
        { workspaceId, categoryId, containerId },
        { method: 'POST' },
      );
      const { containerIds } = await this.#category(workspaceId, categoryId);
      const filed = containerIds.filter((id) => id !== containerId);
      filed.splice(Math.max(0, Math.min(index, filed.length)), 0, containerId);
      await this.#http.request(
        underWorkspace('/categories/{categoryId}/container-order'),
        { workspaceId, categoryId },
        { method: 'PUT', json: filed },
      );
    });
  }

  setContainerHidden(
    workspaceId: string,
    categoryId: string,
    id: string,
    hidden: boolean,
  ): Promise<void> {
    return this.#turn(workspaceId, async () => {
      await this.#http.request(
        hidden
          ? underWorkspace('/categories/{categoryId}/containers/{containerId}/hide')
          : underWorkspace('/categories/{categoryId}/containers/{containerId}/unhide'),
        { workspaceId, categoryId, containerId: id },
        { method: 'PUT' },
      );
    });
  }

  async #category(workspaceId: string, categoryId: string): Promise<Category> {
    const found = (await this.categories(workspaceId)).find((one) => one.id === categoryId);
    if (!found) throw new BackendError(404, { detail: 'no such category' });
    return found;
  }
}
