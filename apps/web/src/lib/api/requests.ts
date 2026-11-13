/**
 * The requests of the server backend that take nothing but HTTP: making an
 * account, what the login page shows of an organization, changing the
 * reader's own and reading their workspaces, and reading the trash.
 * `ServerBackend` is made of these and the rest (`backend.ts`).
 */
import type { Board } from '../logic/wire.ts';
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

import { BackendError } from '../state/backend.ts';
import type { People } from '../state/people-types.ts';

import { boardOf, workspacesOf, userOf, type Schemas } from './convert.ts';
import type { ApiPath, Http, RequestOptions } from './http.ts';
import { ServerPeople } from './people.ts';

/** A path under a workspace, as the contract writes it after `/api/v1/w/{workspaceId}`. */
export type WorkspacePath = ApiPath extends infer P
  ? P extends `/api/v1/w/{workspaceId}${infer Rest}`
    ? Rest
    : never
  : never;

/**
 * The API path of `path` under a workspace: every address under
 * `/api/v1/w/{workspaceId}` is made from this.
 */
export function underWorkspace<P extends WorkspacePath>(path: P): `/api/v1/w/{workspaceId}${P}` {
  return `/api/v1/w/{workspaceId}${path}`;
}

/** How many identifiers `getWorkspaceUsersById` takes at once (its `maxItems`). */
export const USERS_AT_ONCE = 200;

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
  /** Who is in a workspace, and the links that let others in. */
  readonly people: People;
  /** The workspace the reader is in (`enter`): the paths of its boards are under it. */
  #workspaceId: string | undefined;
  /**
   * The workspace of each board read (`learnPlaces`): a board's paths are
   * under its own, also when the reader has entered another since a request
   * on it began (an upload waiting for its turn, a write after a reading).
   */
  readonly #places = new Map<string, string>();

  constructor(http: Http) {
    this.#http = http;
    this.people = new ServerPeople(http);
  }

  /** The requests that name no workspace go to this one from now on. */
  enter(workspaceId: string): void {
    this.#workspaceId = workspaceId;
  }

  /**
   * Keeps the workspace of each board, for the paths of the board
   * (`workspaceOf`). A built-in template is every workspace's: its paths are
   * under the one the reader is in.
   */
  protected learnPlaces(
    boards: readonly { id: string; workspaceId: string; isBuiltin?: boolean }[],
  ): void {
    for (const board of boards) {
      if (!board.isBuiltin) this.#places.set(board.id, board.workspaceId);
    }
  }

  /**
   * The parameters of a path under `/w/{workspaceId}` with the workspace
   * filled in: the board's own when `boardId` names a board read before (in
   * the path, or beside it for a path of a card), else the one the reader is
   * in, unless `workspaceId` names another.
   * Every address under a workspace is made with these.
   */
  protected workspaceOf<P extends Record<string, string>>(
    params: P,
    workspaceId = (params.boardId && this.#places.get(params.boardId)) || this.#workspaceId,
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
      underWorkspace(path),
      this.workspaceOf(params, workspaceId),
      options,
    );
  }

  /** The bytes at `path` under the workspace (a file). */
  protected inWorkspaceBytes(path: WorkspacePath, params: Record<string, string>): Promise<Blob> {
    return this.#http.bytes(underWorkspace(path), this.workspaceOf(params));
  }

  /** The address of `path` under the workspace. */
  protected inWorkspaceUrl(path: WorkspacePath, params: Record<string, string>): string {
    return this.#http.url(underWorkspace(path), this.workspaceOf(params));
  }

  async boards(workspaceId: string): Promise<Board[]> {
    const boards = await this.#http.request<Schemas['Board'][]>(underWorkspace('/boards'), {
      workspaceId,
    });
    this.learnPlaces(boards);
    return boards.map(boardOf);
  }

  async board(boardId: string): Promise<Board> {
    const board = boardOf(
      await this.inWorkspace<Schemas['Board']>('/boards/{boardId}', { boardId }),
    );
    this.learnPlaces([board]);
    return board;
  }

  /**
   * The people of a workspace by their identifiers, each asked for once. The
   * server leaves out whom the reader does not see, and takes at most
   * `USERS_AT_ONCE` identifiers in a request (none is refused): a longer list
   * is asked for in parts.
   */
  async usersById(workspaceId: string, userIds: string[]): Promise<User[]> {
    const ids = [...new Set(userIds)];
    const parts: string[][] = [];
    for (let at = 0; at < ids.length; at += USERS_AT_ONCE) {
      parts.push(ids.slice(at, at + USERS_AT_ONCE));
    }
    const found = await Promise.all(
      parts.map((json) =>
        this.#http.request<Schemas['User'][]>(
          underWorkspace('/users'),
          { workspaceId },
          { method: 'POST', json },
        ),
      ),
    );
    return found.flat().map(userOf);
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
    return this.inWorkspace<Schemas['DeletedItem'][]>(
      '/boards/{boardId}/trash/cards',
      { boardId },
      { query: { ...paging } },
    );
  }

  async deletedBoards(workspaceId: string, paging?: Paging): Promise<DeletedItem[]> {
    return this.#http.request<Schemas['DeletedItem'][]>(
      underWorkspace('/trash/boards'),
      { workspaceId },
      { query: { ...paging } },
    );
  }
}
