/**
 * The requests of the server backend that take nothing but HTTP: making an
 * account, what the login page shows of an organization, changing the
 * reader's own and reading their workspaces, and reading the trash.
 * `ServerBackend` is made of these and the rest (`backend.ts`).
 */
import type {
  AcceptedInvitation,
  DeletedItem,
  InvitationPreview,
  LoginOptions,
  NewAccount,
  Paging,
  Registration,
  WorkspaceInfo,
  TrashReads,
  User,
} from '../state/backend.ts';

import { workspacesOf, userOf, type Schemas } from './convert.ts';
import type { Http } from './http.ts';

/** The account as the server takes it: a display name that is empty is left out. */
function accountOf(account: NewAccount): Schemas['NewAccount'] {
  return {
    username: account.username,
    email: account.email,
    password: account.password,
    ...(account.displayName ? { displayName: account.displayName } : {}),
  };
}

export class ServerRequests implements TrashReads {
  readonly #http: Http;

  constructor(http: Http) {
    this.#http = http;
  }

  async register(registration: Registration): Promise<void> {
    const request: Schemas['RegisterRequest'] = {
      ...accountOf(registration),
      organizationName: registration.organizationName,
    };
    await this.#http.request('/api/v1/register', {}, { method: 'POST', json: request });
  }

  /** The token goes in the body: an address would keep it in logs. */
  async previewInvitation(token: string): Promise<InvitationPreview> {
    const request: Schemas['InvitationToken'] = { token };
    const preview = await this.#http.request<Schemas['InvitationPreview']>(
      '/api/v1/invitations/preview',
      {},
      { method: 'POST', json: request },
    );
    return {
      workspaceName: preview.workspaceName,
      inviterName: preview.inviterName,
      ...(typeof preview.boardTitle === 'string' ? { boardTitle: preview.boardTitle } : {}),
    };
  }

  async acceptInvitation(token: string, account?: NewAccount): Promise<AcceptedInvitation> {
    const request: Schemas['AcceptInvitation'] = {
      token,
      ...(account ? { account: accountOf(account) } : {}),
    };
    const accepted = await this.#http.request<Schemas['AcceptedInvitation']>(
      '/api/v1/invitations/accept',
      {},
      { method: 'POST', json: request },
    );
    return {
      workspaceId: accepted.workspaceId,
      ...(accepted.boardId ? { boardId: accepted.boardId } : {}),
    };
  }

  async loginOptions(orgId: string | undefined): Promise<LoginOptions> {
    const options = await this.#http.request<Schemas['LoginOptions']>(
      '/api/v1/login-options',
      {},
      { query: { org: orgId } },
    );
    return {
      ...(options.organization ? { organization: options.organization } : {}),
      registrationOpen: options.registrationOpen,
    };
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
