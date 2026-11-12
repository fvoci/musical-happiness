/**
 * The requests of the server backend that take nothing but HTTP: making an
 * account, what the login page shows of an organization, changing the
 * reader's own and reading their workspaces, and reading the trash.
 * `ServerBackend` is made of these and the rest (`backend.ts`).
 */
import type {
  DeletedItem,
  LoginOrganization,
  Paging,
  Registration,
  WorkspaceInfo,
  TrashReads,
  User,
} from '../state/backend.ts';

import { BackendError } from '../state/backend.ts';

import { workspacesOf, userOf, type Schemas } from './convert.ts';
import type { ApiPath, Http, RequestOptions } from './http.ts';

/** A path under a workspace, as the contract writes it after `/api/v1/w/{workspaceId}`. */
export type WorkspacePath = ApiPath extends infer P
  ? P extends `/api/v1/w/{workspaceId}${infer Rest}`
    ? Rest
    : never
  : never;

export class ServerRequests implements TrashReads {
  readonly #http: Http;
  /** The workspace the reader is in (`enter`): the paths of its boards are under it. */
  #workspaceId: string | undefined;

  constructor(http: Http) {
    this.#http = http;
  }

  /** The requests that name no workspace go to this one from now on. */
  enter(workspaceId: string): void {
    this.#workspaceId = workspaceId;
  }

  /**
   * The parameters of a path under `/w/{workspaceId}` with the workspace
   * filled in: the one the reader is in, unless `workspaceId` names another.
   * Every address under a workspace is made with these.
   */
  protected workspaceOf<P extends Record<string, string>>(
    params: P,
    workspaceId = this.#workspaceId,
  ): P & { workspaceId: string } {
    if (workspaceId === undefined) {
      throw new BackendError(500, { detail: 'no workspace was entered' });
    }
    return { ...params, workspaceId };
  }

  /** A request to `path` under the workspace (`workspaceOf`). */
  protected inWorkspace<T>(
    path: WorkspacePath,
    params: Record<string, string> = {},
    options: RequestOptions = {},
    workspaceId?: string,
  ): Promise<T> {
    return this.#http.request<T>(
      `/api/v1/w/{workspaceId}${path}`,
      this.workspaceOf(params, workspaceId),
      options,
    );
  }

  /** The bytes at `path` under the workspace (a file). */
  protected inWorkspaceBytes(path: WorkspacePath, params: Record<string, string>): Promise<Blob> {
    return this.#http.bytes(`/api/v1/w/{workspaceId}${path}`, this.workspaceOf(params));
  }

  /** The address of `path` under the workspace. */
  protected inWorkspaceUrl(path: WorkspacePath, params: Record<string, string>): string {
    return this.#http.url(`/api/v1/w/{workspaceId}${path}`, this.workspaceOf(params));
  }

  async register(registration: Registration): Promise<void> {
    const request: Schemas['RegisterRequest'] = {
      username: registration.username,
      email: registration.email,
      password: registration.password,
      ...(registration.displayName ? { displayName: registration.displayName } : {}),
      ...(registration.token ? { token: registration.token } : {}),
    };
    await this.#http.request('/api/v1/register', {}, { method: 'POST', json: request });
  }

  async loginOrganization(orgId: string | undefined): Promise<LoginOrganization | undefined> {
    const options = await this.#http.request<Schemas['LoginOptions']>(
      '/api/v1/login-options',
      {},
      { query: { org: orgId } },
    );
    return options.organization ?? undefined;
  }

  /**
   * Starts a sign-in through the organization's provider, as a request of this
   * page (which another site's page cannot send): the address to send the
   * browser to. The cookie that ties the sign-in to this browser comes with it.
   */
  async startOidc(orgId: string): Promise<string> {
    const started = await this.#http.request<Schemas['OidcStart']>(
      '/api/v1/auth/oidc/start',
      {},
      { method: 'POST', query: { org: orgId } },
    );
    return started.authorizationUrl;
  }

  async patchMe(patch: { displayName: string | null }): Promise<User> {
    const change: Schemas['AccountChange'] = patch;
    return userOf(
      await this.#http.request<Schemas['User']>(
        '/api/v1/users/me',
        {},
        { method: 'PATCH', json: change },
      ),
    );
  }

  async workspaces(): Promise<WorkspaceInfo[]> {
    return workspacesOf(
      await this.#http.request<Schemas['UserWorkspace'][]>('/api/v1/users/me/workspaces'),
    );
  }

  async deletedCards(boardId: string, paging?: Paging): Promise<DeletedItem[]> {
    return this.inWorkspace<Schemas['DeletedItem'][]>(
      '/boards/{boardId}/trash/cards',
      { boardId },
      { query: { ...paging } },
    );
  }

  async deletedBoards(workspaceId: string, paging?: Paging): Promise<DeletedItem[]> {
    return this.#http.request<Schemas['DeletedItem'][]>(
      '/api/v1/w/{workspaceId}/trash/boards',
      { workspaceId },
      { query: { ...paging } },
    );
  }
}
