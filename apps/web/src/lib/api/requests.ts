/**
 * The requests of the server backend that take nothing but HTTP: making an
 * account, what the login page shows of an organization, signing in
 * through its provider and what is connected to the account, changing the
 * reader's own and reading their workspaces, and reading the trash.
 * `ServerBackend` is made of these and the rest (`backend.ts`).
 */
import type {
  DeletedItem,
  LinkedIdentity,
  LoginOrganization,
  Paging,
  Registration,
  WorkspaceInfo,
  TrashReads,
  User,
} from '../state/backend.ts';

import { workspacesOf, userOf, type Schemas } from './convert.ts';
import type { Http } from './http.ts';

export class ServerRequests implements TrashReads {
  readonly #http: Http;

  constructor(http: Http) {
    this.#http = http;
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
   * With `linkPassword` it connects the provider to the reader's account.
   */
  async startOidc(orgId: string, linkPassword?: string): Promise<string> {
    const body: Schemas['OidcStartRequest'] | undefined =
      linkPassword === undefined ? undefined : { linkPassword };
    const started = await this.#http.request<Schemas['OidcStart']>(
      '/api/v1/auth/oidc/start',
      {},
      { method: 'POST', query: { org: orgId }, json: body },
    );
    return started.authorizationUrl;
  }

  async identities(): Promise<LinkedIdentity[]> {
    return this.#http.request<Schemas['LinkedIdentity'][]>('/api/v1/users/me/identities');
  }

  async unlinkIdentity(identity: LinkedIdentity, password: string): Promise<void> {
    const request: Schemas['UnlinkRequest'] = {
      issuer: identity.issuer,
      subject: identity.subject,
      password,
    };
    await this.#http.request(
      '/api/v1/users/me/identities',
      {},
      { method: 'DELETE', json: request },
    );
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
    return this.#http.request<Schemas['DeletedItem'][]>(
      '/api/v1/boards/{boardId}/trash/cards',
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
