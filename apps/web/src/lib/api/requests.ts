/**
 * The requests of the server backend that take nothing but HTTP: making an
 * account, what the login page shows of an organization, signing in
 * through its provider and what is connected to the account, changing the
 * reader's own and reading their workspaces, reading files, and reading the
 * trash.
 * `ServerBackend` is made of these and the rest (`backend.ts`).
 */
import type { FileDetails } from '../editor/files.ts';
import type { NameChange } from '../logic/person-names.ts';
import type { Container } from '../logic/wire.ts';
import type {
  AcceptedInvitation,
  AccountDeletion,
  DeletedItem,
  LinkedIdentity,
  InvitationPreview,
  LoginOptions,
  NewAccount,
  Paging,
  Registration,
  WorkspaceInfo,
  TrashReads,
  User,
} from '../state/backend.ts';

import type { Registered } from '../state/account-types.ts';
import { BackendError } from '../state/backend.ts';
import type { BodyVersions } from '../state/body-versions.ts';
import type { FileVersions } from '../state/file-versions.ts';
import type { People } from '../state/people-types.ts';
import type { References } from '../state/references.ts';

import { ServerBodyVersions } from './body-versions.ts';
import { ServerFileVersions } from './file-versions.ts';
import { containerOf, workspacesOf, userOf, type Schemas } from './convert.ts';
import type { ApiPath, Http, RequestOptions } from './http.ts';
import { ServerPeople } from './people.ts';
import { ServerReferences } from './references.ts';
import { ServerWikis } from './wikis.ts';

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

/** The trash as the contract lists it: a board's `kind` where it says one. */
function deletedItemsOf(items: readonly Schemas['DeletedItem'][]): DeletedItem[] {
  return items.map(({ kind, ...item }) => (kind ? { ...item, kind } : item));
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
  /** The wikis and their pages, under the workspace of each wiki (`workspaceOf`). */
  readonly wiki: ServerWikis;
  /** The versions kept of the bodies of cards. */
  readonly bodyVersions: BodyVersions;
  readonly fileVersions: FileVersions;
  /** What bodies name, as the reader may see it. */
  readonly references: References;
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
    this.bodyVersions = new ServerBodyVersions((path, params, options) =>
      this.inWorkspace(path, params, options),
    );
    this.fileVersions = new ServerFileVersions(
      (path, params, options) => this.inWorkspace(path, params, options),
      (path, params, signal) => this.inWorkspaceBytes(path, params, signal),
    );
    this.references = new ServerReferences((path, params, options, workspaceId) =>
      this.inWorkspace(path, params, options, workspaceId),
    );
    this.wiki = new ServerWikis(this.#inWiki.bind(this), (wikis) => {
      this.learnPlaces(wikis);
    });
  }

  /** A request under the workspace of the wiki `params.wikiId` names (`workspaceOf`). */
  #inWiki<T>(
    path: WorkspacePath,
    params: Record<string, string>,
    options: RequestOptions = {},
    workspaceId?: string,
  ): Promise<T> {
    return this.inWorkspace<T>(
      path,
      { ...params, containerId: params.wikiId ?? '' },
      options,
      workspaceId,
    );
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
   * filled in: the board's or wiki's own when `containerId` or `boardId` names
   * one read before (in the path, or beside it for a path of a card), else the
   * one the reader is in, unless `workspaceId` names another.
   * Every address under a workspace is made with these.
   */
  protected workspaceOf<P extends Record<string, string>>(
    params: P,
    workspaceId = this.#placeOf(params.containerId || params.boardId) || this.#workspaceId,
  ): P & { workspaceId: string } {
    if (workspaceId === undefined) {
      throw new BackendError(500, { detail: 'no workspace was entered' });
    }
    return { ...params, workspaceId };
  }

  /** The workspace of the board or wiki `id`, if it was read before. */
  #placeOf(id: string | undefined): string | undefined {
    return id ? this.#places.get(id) : undefined;
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
  protected inWorkspaceBytes(
    path: WorkspacePath,
    params: Record<string, string>,
    signal?: AbortSignal,
  ): Promise<Blob> {
    return this.#http.bytes(underWorkspace(path), this.workspaceOf(params), {}, signal);
  }

  /** The address of `path` under the workspace. */
  protected inWorkspaceUrl(path: WorkspacePath, params: Record<string, string>): string {
    return this.#http.url(underWorkspace(path), this.workspaceOf(params));
  }

  async boards(workspaceId: string): Promise<Container[]> {
    const boards = await this.#http.request<Schemas['Container'][]>(underWorkspace('/boards'), {
      workspaceId,
    });
    this.learnPlaces(boards);
    return boards.map(containerOf);
  }

  async container(containerId: string): Promise<Container> {
    const board = containerOf(
      await this.inWorkspace<Schemas['Container']>('/containers/{containerId}', {
        containerId,
      }),
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

  async register(registration: Registration, language = ''): Promise<Registered> {
    const request: Schemas['RegisterRequest'] = {
      ...accountOf(registration),
      // Left out for none: the account starts in its own space alone.
      ...(registration.organizationName ? { organizationName: registration.organizationName } : {}),
      // The language of the confirmation mail, where the server sends one.
      ...(language ? { language } : {}),
    };
    // 201 with the account, or 202 with nothing: the address is to be confirmed first.
    const made = await this.#http.request<Schemas['User'] | undefined>(
      '/api/v1/register',
      {},
      { method: 'POST', json: request },
    );
    return made === undefined ? 'waiting' : 'made';
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
      ...(typeof preview.containerTitle === 'string'
        ? { containerTitle: preview.containerTitle }
        : {}),
      ...(preview.containerKind ? { containerKind: preview.containerKind } : {}),
    };
  }

  async acceptInvitation(
    token: string,
    account?: NewAccount,
    language = '',
  ): Promise<AcceptedInvitation | 'waiting'> {
    const request: Schemas['AcceptInvitation'] = {
      token,
      ...(language ? { language } : {}),
      ...(account ? { account: accountOf(account) } : {}),
    };
    // 202 with nothing: the new account waits for its address to be confirmed.
    const accepted = await this.#http.request<Schemas['AcceptedInvitation'] | undefined>(
      '/api/v1/invitations/accept',
      {},
      { method: 'POST', json: request },
    );
    if (accepted === undefined) return 'waiting';
    return {
      workspaceId: accepted.workspaceId,
      ...(accepted.containerId ? { containerId: accepted.containerId } : {}),
      ...(accepted.containerKind ? { containerKind: accepted.containerKind } : {}),
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
      mailEnabled: options.mailEnabled,
      emailVerification: options.emailVerification,
      organizationRequired: options.organizationRequired,
    };
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

  async startOidcCarryOver(orgId: string): Promise<string> {
    const started = await this.#http.request<Schemas['OidcStart']>(
      '/api/v1/auth/oidc/held/carry-over',
      {},
      { method: 'POST', query: { org: orgId } },
    );
    return started.authorizationUrl;
  }

  async finishHeldSignIn(orgId: string): Promise<void> {
    await this.#http.request(
      '/api/v1/auth/oidc/held/sign-in',
      {},
      { method: 'POST', query: { org: orgId } },
    );
  }

  async identities(): Promise<LinkedIdentity[]> {
    return this.#http.request<Schemas['LinkedIdentity'][]>('/api/v1/users/me/identities');
  }

  async deletion(): Promise<AccountDeletion> {
    return this.#http.request<Schemas['AccountDeletion']>('/api/v1/users/me/deletion');
  }

  async requestDeletion(username: string, password: string): Promise<number> {
    const request: Schemas['DeleteAccountRequest'] = password
      ? { username, password }
      : { username };
    const asked = await this.#http.request<Schemas['AccountDeletionAsked']>(
      '/api/v1/users/me/deletion',
      {},
      { method: 'POST', json: request },
    );
    return asked.deleteAfter;
  }

  async cancelDeletion(): Promise<void> {
    await this.#http.request('/api/v1/users/me/deletion', {}, { method: 'DELETE' });
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

  async patchMe(patch: NameChange): Promise<User> {
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

  /** The bytes of a file (`getFile`), until `signal` gives the reading up. */
  fileBytes(containerId: string, fileId: string, signal?: AbortSignal): Promise<Blob> {
    const params = { containerId, fileId };
    return this.inWorkspaceBytes('/containers/{containerId}/files/{fileId}', params, signal);
  }

  /** The name and size of a file (`getFileInfo`). */
  async fileDetails(containerId: string, fileId: string): Promise<FileDetails> {
    const params = { containerId, fileId };
    const info = await this.inWorkspace<Schemas['FileInfo']>(
      '/containers/{containerId}/files/{fileId}/info',
      params,
    );
    return { name: info.name, size: info.size };
  }

  /** The file's bytes, from the page's own origin: the session's cookie goes along. */
  fileUrl(containerId: string, fileId: string): string | undefined {
    return this.inWorkspaceUrl('/containers/{containerId}/files/{fileId}', { containerId, fileId });
  }

  async deletedCards(boardId: string, paging?: Paging): Promise<DeletedItem[]> {
    return deletedItemsOf(
      await this.inWorkspace<Schemas['DeletedItem'][]>(
        '/boards/{boardId}/trash/cards',
        { boardId },
        { query: { ...paging } },
      ),
    );
  }

  async deletedContainers(workspaceId: string, paging?: Paging): Promise<DeletedItem[]> {
    const items = await this.#http.request<Schemas['DeletedItem'][]>(
      underWorkspace('/trash/containers'),
      { workspaceId },
      { query: { ...paging } },
    );
    return deletedItemsOf(items);
  }
}
