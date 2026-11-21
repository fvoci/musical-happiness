/**
 * The backend that calls the server: the data boundary
 * (`lib/state/backend.ts`) over the server's API (`contracts/openapi.json`).
 */
import type { CardPreview } from '../editor/card-preview.ts';
import type { DocNode } from '../editor/schema.ts';
import type {
  BlockPatch,
  Container,
  ContainerMember,
  ContainerPatch,
  BoardView,
} from '../logic/wire.ts';
import { openRoomLater } from '../state/doc-opener.ts';
import { allPages } from '../state/page-reads.ts';
import { applyViewPatch } from '../state/patches.ts';
import {
  BackendError,
  refusedMember,
  type Account,
  type AccountSettings,
  type AccountSettingsPatch,
  type Attachment,
  type Backend,
  type BoardDraft,
  type ContainerRole,
  type BodyDoc,
  type BodyDocHears,
  type BodyWrite,
  type CardDetail,
  type CardDraft,
  type CardPatch,
  type CardSummary,
  type Category,
  type CategoryDraft,
  type CategoryPatch,
  type ChangeListener,
  type Comment,
  type CommentVersion,
  type DownloadedFile,
  type DownloadOptions,
  type Sharing,
  type SharingChange,
  type UploadedFile,
  type UploadOptions,
  type User,
  type ViewDraft,
} from '../state/backend.ts';

import {
  attachmentOf,
  containerOf,
  cardDetailOf,
  cardOf,
  categoryOf,
  commentOf,
  commentVersionOf,
  memberOf,
  newBoardAndViewOf,
  newCardOf,
  containerPatchOf,
  previewsOf,
  settingsChangeOf,
  rolesOf,
  settingsOf,
  sharingOf,
  userOf,
  viewOf,
  wireBody,
  type Schemas,
} from './convert.ts';
import { ClientConfig, ServerContentSearch } from './content-search.ts';
import { downloadWithProgress } from './download.ts';
import { Http } from './http.ts';
import { LiveChanges } from './live.ts';
import { ServerRequests, underWorkspace } from './requests.ts';
import { makeWelcomeBoard, templatesOf } from './templates.ts';
import { importInParts, tooLarge, uploadInParts } from './upload.ts';

/** How many files go up at once; the server allows a reader 8 open uploads. */
const UPLOADS_AT_ONCE = 3;

export class ServerBackend extends ServerRequests implements Backend, Account {
  readonly #http: Http;
  /** The last change of each card or view on its way, settled either way (`#inTurn`). */
  readonly #writes = new Map<string, Promise<void>>();

  readonly #live: LiveChanges;
  readonly presence: Backend['presence'];
  readonly search: Backend['search'];
  /** The server's settings for clients, read once. */
  readonly #config: ClientConfig;

  /** `basePath` is the path the app is served under. */
  constructor(basePath: string) {
    const http = new Http(basePath);
    super(http);
    this.#http = http;
    this.#config = new ClientConfig(http);
    this.search = new ServerContentSearch(http, this.#config);
    // The page's own origin: the browser sends the session's cookie with it.
    const socket = new URL(`${basePath}/ws`, location.href);
    socket.protocol = socket.protocol === 'https:' ? 'wss:' : 'ws:';
    this.#live = new LiveChanges(socket.href, {
      members: (containerId) => this.members(containerId),
    });
    this.presence = this.#live.presence;
  }

  // ------------------------------------------------------------ account

  /**
   * The server takes the name first and the email address when no user has
   * that name; a name may have an `@` in it. So the text goes as both.
   */
  async signIn(login: string, password: string): Promise<void> {
    const request: Schemas['LoginRequest'] = { username: login, email: login, password };
    await this.#http.request('/api/v1/login', {}, { method: 'POST', json: request });
    // The page's connection was opened with the old cookie, or none.
    this.#live.restart();
  }

  async signOut(): Promise<void> {
    await this.#http.request('/api/v1/logout', {}, { method: 'POST' });
  }

  /** As after signing in: the page's connection must not be one opened under the old cookie. */
  override async confirmPassword(password: string): Promise<void> {
    await super.confirmPassword(password);
    this.#live.restart();
  }

  // ------------------------------------------- the reader and their workspace

  /**
   * The people of the workspace. The server answers at most ten of them
   * (`getWorkspaceUsers` without a search; `docs/contract-assumptions.md`, D2).
   */
  async users(workspaceId: string): Promise<User[]> {
    const users = await this.#http.request<Schemas['User'][]>(underWorkspace('/users'), {
      workspaceId,
    });
    return users.map(userOf);
  }

  async settings(): Promise<AccountSettings> {
    return settingsOf(
      await this.#http.request<Schemas['UserSettings']>('/api/v1/users/me/settings'),
    );
  }

  async patchSettings(patch: AccountSettingsPatch): Promise<AccountSettings> {
    const settings = await this.#http.request<Schemas['UserSettings']>(
      '/api/v1/users/me/settings',
      {},
      { method: 'PATCH', json: settingsChangeOf(patch) },
    );
    return settingsOf(settings);
  }

  async screenState(): Promise<Record<string, string>> {
    return this.#http.request<Record<string, string>>('/api/v1/users/me/client-state');
  }

  async patchScreenState(updated: Record<string, string>, deleted: string[] = []): Promise<void> {
    const patch: Schemas['UserPreferencesPatch'] = {
      updatedFields: updated,
      deletedFields: deleted,
    };
    await this.#http.request('/api/v1/users/me/client-state', {}, { method: 'PATCH', json: patch });
  }

  // ------------------------------------------------------------- boards

  /**
   * Makes the board and its first view in one request
   * (`createBoardsAndBlocks`): all or none. The identifiers sent only tie
   * the view to the board; the server makes the real ones.
   */
  async createBoard(
    workspaceId: string,
    draft: BoardDraft,
    firstView: ViewDraft,
  ): Promise<{ board: Container; view: BoardView }> {
    const request = newBoardAndViewOf(draft, firstView);
    const made = await this.inWorkspace<Schemas['ContainersAndBlocks']>(
      '/boards-and-blocks',
      {},
      { method: 'POST', json: request },
      workspaceId,
    );
    const [board] = made.containers;
    const view = made.blocks.find((block) => block.type === 'view');
    if (!board || !view) throw new BackendError(500, { detail: 'the server made no board' });
    return { board: containerOf(board), view: viewOf(view) };
  }

  /** Puts the card properties in an order (`reorderCardProperties`); the answer is the order. */
  async reorderCardProperties(boardId: string, propertyIds: string[]): Promise<string[]> {
    return this.inWorkspace<string[]>(
      '/boards/{boardId}/property-order',
      { boardId },
      { method: 'PUT', json: propertyIds },
    );
  }

  /**
   * Changes the board itself. A minimum role the contract does not have
   * (none, or admin) is refused here as the server would refuse it.
   */
  async patchContainer(id: string, patch: ContainerPatch): Promise<Container> {
    const request = containerPatchOf(patch);
    return containerOf(
      await this.inWorkspace<Schemas['Container']>(
        '/containers/{containerId}',
        { containerId: id },
        { method: 'PATCH', json: request },
      ),
    );
  }

  /** `searchWorkspaceBoards` by title: the boards of the workspace. */
  async searchBoards(workspaceId: string, query: string): Promise<Container[]> {
    const boards = await this.#http.request<Schemas['Container'][]>(
      underWorkspace('/boards/search'),
      { workspaceId },
      { query: { q: query } },
    );
    return boards.map(containerOf);
  }

  templates(workspaceId: string): Promise<Container[]> {
    return templatesOf(this.#http, workspaceId);
  }

  /** `duplicateContainer` into the workspace `to` (`toWorkspace`). The copy is the first board of the answer. */
  async duplicateContainer(id: string, asTemplate: boolean, to: string): Promise<Container> {
    const made = await this.inWorkspace<Schemas['ContainersAndBlocks']>(
      '/containers/{containerId}/duplicate',
      { containerId: id },
      { method: 'POST', query: { asTemplate: String(asTemplate), toWorkspace: to } },
    );
    const [board] = made.containers;
    if (!board) throw new BackendError(500, { detail: 'the server made no copy' });
    return containerOf(board);
  }

  onboard(workspaceId: string): Promise<string> {
    return makeWelcomeBoard(this.#http, workspaceId);
  }

  /** To the server's trash, with its blocks and members (`deleteContainer`, board admins only). */
  async deleteContainer(id: string): Promise<void> {
    await this.inWorkspace('/containers/{containerId}', { containerId: id }, { method: 'DELETE' });
  }

  /** Out of the server's trash, filed where each person had it (the category messages tell). */
  async restoreContainer(id: string): Promise<Container> {
    return containerOf(
      await this.inWorkspace<Schemas['Container']>(
        '/containers/{containerId}/undelete',
        { containerId: id },
        { method: 'POST' },
      ),
    );
  }

  async members(id: string): Promise<ContainerMember[]> {
    const path = '/containers/{containerId}/members';
    const members = await this.inWorkspace<Schemas['ContainerMember'][]>(path, { containerId: id });
    return members.map(memberOf);
  }

  /** `addMember` with exactly the one role, as the server gives roles. */
  async addMember(id: string, userId: string, role: ContainerRole): Promise<ContainerMember> {
    const request: Schemas['NewMember'] = { userId, ...rolesOf(role) };
    return memberOf(
      await this.inWorkspace<Schemas['ContainerMember']>(
        '/containers/{containerId}/members',
        { containerId: id },
        { method: 'POST', json: request },
      ),
    );
  }

  async setMemberRole(id: string, userId: string, role: ContainerRole): Promise<ContainerMember> {
    return memberOf(
      await this.inWorkspace<Schemas['ContainerMember']>(
        '/containers/{containerId}/members/{userId}',
        { containerId: id, userId },
        { method: 'PUT', json: rolesOf(role) },
      ),
    );
  }

  async removeMember(id: string, userId: string): Promise<void> {
    await this.inWorkspace(
      '/containers/{containerId}/members/{userId}',
      { containerId: id, userId },
      { method: 'DELETE' },
    );
  }

  async sharing(id: string): Promise<Sharing> {
    const path = '/containers/{containerId}/sharing';
    return sharingOf(await this.inWorkspace<Schemas['Sharing']>(path, { containerId: id }));
  }

  async setSharing(id: string, change: SharingChange): Promise<Sharing> {
    const json: Schemas['SharingRequest'] = {
      enabled: change.enabled,
      ...(change.regenerate ? { regenerate: true } : {}),
    };
    const path = '/containers/{containerId}/sharing';
    const options = { method: 'POST', json } as const;
    return sharingOf(
      await this.inWorkspace<Schemas['Sharing']>(path, { containerId: id }, options),
    );
  }

  /** `getWorkspaceUsers` with a search: ten at most, whatever the case (D2). */
  async searchUsers(workspaceId: string, query: string): Promise<User[]> {
    const users = await this.#http.request<Schemas['User'][]>(
      underWorkspace('/users'),
      { workspaceId },
      { query: { search: query } },
    );
    return users.map(userOf);
  }

  /** Joins an open board (`joinContainer`), with its minimum role. */
  async joinContainer(id: string): Promise<ContainerMember> {
    return memberOf(
      await this.inWorkspace<Schemas['ContainerMember']>(
        '/containers/{containerId}/join',
        { containerId: id },
        { method: 'POST' },
      ),
    );
  }

  // ------------------------------------------------------------ sidebar

  async categories(workspaceId: string): Promise<Category[]> {
    const categories = await this.#http.request<Schemas['CategoryContainers'][]>(
      underWorkspace('/categories'),
      { workspaceId },
    );
    return categories.map(categoryOf);
  }

  /**
   * The sidebar's writes of a workspace go one after another: a rename or a
   * filing reads the category first and sends what it read, so a write
   * begun before the last one ended would send what that one changed back.
   */
  #sidebarTurn<T>(workspaceId: string, write: () => Promise<T>): Promise<T> {
    return this.#inTurn(`sidebar ${workspaceId}`, write);
  }

  /** A new category of the reader, first among theirs (the server puts it there). */
  createCategory(workspaceId: string, draft: CategoryDraft): Promise<Category> {
    return this.#sidebarTurn(workspaceId, () => this.#createCategory(workspaceId, draft));
  }

  async #createCategory(workspaceId: string, draft: CategoryDraft): Promise<Category> {
    // Read each time: another tab may have signed in as somebody else.
    const { id: userId } = await this.me();
    const request: Schemas['NewCategory'] = { name: draft.name, userId, workspaceId };
    const made = await this.#http.request<Schemas['Category']>(
      underWorkspace('/categories'),
      { workspaceId },
      { method: 'POST', json: request },
    );
    return categoryOf({ ...made, containerMetadata: [] });
  }

  /**
   * Changes a category by `updateCategory`, which takes its name and its
   * folding together: the one the patch leaves is read first. A default
   * category keeps its name (the server answers 200 and leaves it).
   */
  patchCategory(workspaceId: string, id: string, patch: CategoryPatch): Promise<Category> {
    return this.#sidebarTurn(workspaceId, () => this.#patchCategory(workspaceId, id, patch));
  }

  async #patchCategory(
    workspaceId: string,
    categoryId: string,
    patch: CategoryPatch,
  ): Promise<Category> {
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
    // The answer is the category alone; its boards are as they were read.
    return {
      ...categoryOf({ ...updated, containerMetadata: [] }),
      containerIds: before.containerIds,
      hiddenContainerIds: before.hiddenContainerIds,
    };
  }

  deleteCategory(workspaceId: string, categoryId: string): Promise<void> {
    return this.#sidebarTurn(workspaceId, async () => {
      await this.#http.request(
        underWorkspace('/categories/{categoryId}'),
        { workspaceId, categoryId },
        { method: 'DELETE' },
      );
    });
  }

  reorderCategories(workspaceId: string, categoryIds: string[]): Promise<void> {
    return this.#sidebarTurn(workspaceId, async () => {
      await this.#http.request(
        underWorkspace('/category-order'),
        { workspaceId },
        { method: 'PUT', json: categoryIds },
      );
    });
  }

  /**
   * Files a board under a category and puts it at its place: two requests,
   * since the server files a board (`setContainerCategory`) without a place and
   * orders a category's boards (`reorderCategoryContainers`) apart. The order is
   * made from the category as it is after the filing, read again.
   */
  moveContainerToCategory(
    workspaceId: string,
    id: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
    return this.#sidebarTurn(workspaceId, () =>
      this.#moveContainerToCategory(workspaceId, id, categoryId, index),
    );
  }

  async #moveContainerToCategory(
    workspaceId: string,
    containerId: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
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
  }

  setContainerHidden(
    workspaceId: string,
    categoryId: string,
    id: string,
    hidden: boolean,
  ): Promise<void> {
    return this.#sidebarTurn(workspaceId, async () => {
      await this.#http.request(
        hidden
          ? underWorkspace('/categories/{categoryId}/containers/{containerId}/hide')
          : underWorkspace('/categories/{categoryId}/containers/{containerId}/unhide'),
        { workspaceId, categoryId, containerId: id },
        { method: 'PUT' },
      );
    });
  }

  /** One of the reader's categories as the server has it now: 404 for none. */
  async #category(workspaceId: string, categoryId: string): Promise<Category> {
    const found = (await this.categories(workspaceId)).find((one) => one.id === categoryId);
    if (!found) {
      throw new BackendError(404, { detail: 'no such category' });
    }
    return found;
  }

  // ------------------------------------------------------- views, cards

  async views(boardId: string): Promise<BoardView[]> {
    const blocks = await this.inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { query: { type: 'view' } },
    );
    return blocks.map(viewOf);
  }

  /** A view is a block of the board (`createBlocks`). */
  async createView(boardId: string, draft: ViewDraft): Promise<BoardView> {
    // A block right on its board has no parent (the server's way).
    const block: Schemas['NewBlock'] = {
      containerId: boardId,
      type: 'view',
      title: draft.title,
      fields: draft.fields,
      createAt: 1,
      updateAt: 1,
    };
    const [made] = await this.inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { detail: 'the server made no view' });
    return viewOf(made);
  }

  /**
   * Changes a view through its block. The view is read first (so that a
   * block that is not a view is not changed), the patch sent, and the view
   * read again, since the server answers 204 alone. `updatedFields`
   * replaces a field whole, as the screens' patch means. The changes of one
   * view go one after another (`#inTurn`).
   */
  patchView(boardId: string, viewId: string, patch: BlockPatch): Promise<BoardView> {
    return this.#inTurn(viewId, () => this.#patchView(boardId, viewId, patch));
  }

  async #patchView(boardId: string, viewId: string, patch: BlockPatch): Promise<BoardView> {
    const before = await this.#view(boardId, viewId);
    const { deleteAt: _deleteAt, ...request } = patch;
    await this.inWorkspace(
      '/boards/{boardId}/blocks/{blockId}',
      { boardId, blockId: viewId },
      { method: 'PATCH', json: request },
    );
    try {
      return await this.#view(boardId, viewId);
    } catch {
      // Saved; only reading it back failed. What was sent is what it is.
      return applyViewPatch(before, patch);
    }
  }

  /** A view of this board, by its block: 404 for another block or none. */
  async #view(boardId: string, viewId: string): Promise<BoardView> {
    const [view] = await this.inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { query: { blockId: viewId } },
    );
    if (view?.type !== 'view') {
      throw new BackendError(404, { detail: 'no such view on this board' });
    }
    return viewOf(view);
  }

  /** Every card of the board, read page after page. */
  async cards(boardId: string): Promise<CardSummary[]> {
    return (await this.#everyCard(boardId, {})).map(cardOf);
  }

  /** The previews of the cards (`getCards` with `preview=true`); a card without one has none. */
  async cardPreviews(boardId: string): Promise<Map<string, CardPreview>> {
    return previewsOf(await this.#everyCard(boardId, { preview: 'true' }));
  }

  #everyCard(boardId: string, query: Record<string, string>): Promise<Schemas['Card'][]> {
    const path = '/boards/{boardId}/cards';
    return allPages((paging) =>
      this.inWorkspace<Schemas['Card'][]>(path, { boardId }, { query: { ...paging, ...query } }),
    );
  }

  /** A card with its body. */
  async card(boardId: string, cardId: string): Promise<CardDetail> {
    return cardDetailOf(await this.#card(boardId, cardId));
  }

  /**
   * A card of this board. The route finds a card on any board the reader
   * may see; a card of another board is not this board's (as the memory
   * backend says): 404.
   */
  async #card(boardId: string, cardId: string): Promise<Schemas['Card']> {
    // The board names the workspace (`workspaceOf`); the path does not.
    const card = await this.inWorkspace<Schemas['Card']>('/cards/{cardId}', { cardId, boardId });
    if (card.boardId !== boardId) {
      throw new BackendError(404, { detail: 'no such card on this board' });
    }
    return card;
  }

  async createCard(boardId: string, draft: CardDraft): Promise<CardSummary> {
    const card = await this.inWorkspace<Schemas['Card']>(
      '/boards/{boardId}/cards',
      { boardId },
      { method: 'POST', json: newCardOf(draft) },
    );
    return cardOf(card);
  }

  /**
   * Changes a card: the title, the icon, and the values the patch names,
   * set or cleared. The server merges them into what it keeps
   * (`updatedProperties`, `deletedProperties`), so somebody else's change
   * of another value at about the same time stays. The changes of one card
   * still go one after another, so that two quick ones of the same value
   * arrive in the order they were made.
   */
  patchCard(boardId: string, cardId: string, patch: CardPatch): Promise<CardSummary> {
    return this.#inTurn(cardId, () => this.#patchCard(boardId, cardId, patch));
  }

  /**
   * Runs `write` once the writes before it to the same block have ended,
   * however they ended. The server keeps whichever write of a block arrives
   * last; two quick ones must not pass each other on the way.
   */
  #inTurn<T>(blockId: string, write: () => Promise<T>): Promise<T> {
    const before = this.#writes.get(blockId) ?? Promise.resolve();
    const running = before.then(write);
    const settled = running.then(
      () => undefined,
      () => undefined,
    );
    this.#writes.set(blockId, settled);
    void settled.then(() => {
      if (this.#writes.get(blockId) === settled) this.#writes.delete(blockId);
    });
    return running;
  }

  async #patchCard(boardId: string, cardId: string, patch: CardPatch): Promise<CardSummary> {
    const request: Schemas['CardPatch'] = {};
    if (patch.title !== undefined) request.title = patch.title;
    if (patch.icon !== undefined) request.icon = patch.icon;
    const updated = patch.updatedProperties ?? {};
    if (Object.keys(updated).length > 0) request.updatedProperties = updated;
    const deleted = patch.deletedProperties ?? [];
    if (deleted.length > 0) request.deletedProperties = deleted;
    const card = await this.inWorkspace<Schemas['Card']>(
      '/cards/{cardId}',
      { cardId, boardId },
      { method: 'PATCH', json: request },
    );
    return cardOf(card);
  }

  /** The comments of a card: blocks of the type `comment` under it. */
  async comments(boardId: string, cardId: string): Promise<Comment[]> {
    return (await this.#children(boardId, cardId, 'comment')).map(commentOf);
  }

  /**
   * A comment is a block under the card with the text as its body
   * (`docs/contract-pending.md`, 1). The contract's `NewBlock` does not list
   * `body`, but the server takes it there (`docs/contract-assumptions.md`, H8).
   */
  async createComment(boardId: string, cardId: string, body: DocNode): Promise<Comment> {
    const block = {
      containerId: boardId,
      parentId: cardId,
      type: 'comment',
      title: '',
      fields: {},
      createAt: 1,
      updateAt: 1,
      body: wireBody(body),
    } satisfies Schemas['NewBlock'] & { body: Schemas['Body'] };
    const [made] = await this.inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { detail: 'the server made no comment' });
    return commentOf(made);
  }

  /**
   * Replaces a comment's body through its block, with the version it was
   * read at (409 when it changed since). The server answers 204 alone, so
   * the comment is read again; if only that fails, what was sent is what it
   * is. The changes of one comment go in turn.
   */
  patchComment(boardId: string, commentId: string, write: BodyWrite): Promise<Comment> {
    if (!write.body) {
      // The server would take it and leave a comment with no text.
      return Promise.reject(
        new BackendError(400, {
          detail: 'a comment needs a text',
          errors: [refusedMember('body', 'a comment needs a text', 'required')],
        }),
      );
    }
    return this.#inTurn(commentId, async () => {
      const before = await this.#comment(boardId, commentId);
      const patch: Schemas['BlockPatch'] = {
        body: write.body ? wireBody(write.body) : null,
        readAt: write.readAt,
      };
      await this.inWorkspace(
        '/boards/{boardId}/blocks/{blockId}',
        { boardId, blockId: commentId },
        { method: 'PATCH', json: patch },
      );
      try {
        return await this.#comment(boardId, commentId);
      } catch {
        return { ...before, body: write.body };
      }
    });
  }

  /** A comment's earlier texts (`getCommentVersions`), the first first. */
  async commentVersions(boardId: string, commentId: string): Promise<CommentVersion[]> {
    const versions = await this.inWorkspace<Schemas['CommentVersion'][]>(
      '/boards/{boardId}/blocks/{blockId}/versions',
      { boardId, blockId: commentId },
    );
    return versions.map(commentVersionOf);
  }

  /** A comment of this board, by its block: 404 for another block or none. */
  async #comment(boardId: string, commentId: string): Promise<Comment> {
    const [block] = await this.inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { query: { blockId: commentId } },
    );
    if (block?.type !== 'comment') {
      throw new BackendError(404, { detail: 'no such comment on this board' });
    }
    return commentOf(block);
  }

  /** The attachments of a card (`attachmentOf`). */
  async attachments(boardId: string, cardId: string): Promise<Attachment[]> {
    return (await this.#children(boardId, cardId, 'attachment')).map(attachmentOf);
  }

  /** The blocks of one type right under a block. */
  async #children(boardId: string, parentId: string, type: string): Promise<Schemas['Block'][]> {
    return this.inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { query: { parentId, type } },
    );
  }

  /**
   * Attaches an uploaded file: a block under the card that names the file
   * (`fields.fileId`), with the name as its title. The block also keeps the
   * size and type, which the server's file record lacks (`getFileInfo` has
   * no type).
   */
  async createAttachment(boardId: string, cardId: string, file: UploadedFile): Promise<Attachment> {
    const block: Schemas['NewBlock'] = {
      containerId: boardId,
      parentId: cardId,
      type: 'attachment',
      title: file.name,
      fields: { fileId: file.fileId, size: file.size, contentType: file.contentType },
      createAt: 1,
      updateAt: 1,
    };
    const [made] = await this.inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { detail: 'the server made no attachment' });
    return attachmentOf(made);
  }

  /** To the server's trash, with what is under it (204). */
  async deleteBlock(boardId: string, blockId: string): Promise<void> {
    await this.inWorkspace(
      '/boards/{boardId}/blocks/{blockId}',
      { boardId, blockId },
      { method: 'DELETE' },
    );
  }

  /**
   * Out of the server's trash, with what went there with it. The server
   * tells only the block itself to the others (`contracts/websocket.md`);
   * the caller reads what it shows again.
   */
  async restoreBlock(boardId: string, blockId: string): Promise<void> {
    await this.inWorkspace(
      '/boards/{boardId}/blocks/{blockId}/undelete',
      { boardId, blockId },
      { method: 'POST' },
    );
  }

  /**
   * Uploads a file in parts (`upload.ts`). A file over the server's limit
   * (`getClientConfig`'s `maxFileBytes`) is refused before anything is sent,
   * with the limit and the reason, as the server would refuse it (413).
   */
  async upload(containerId: string, file: File, options?: UploadOptions): Promise<UploadedFile> {
    const limit = (await this.#config.read()).maxFileBytes;
    if (file.size > limit) {
      throw tooLarge('file', limit);
    }
    // A few at a time: the server keeps only so many open uploads of a reader (429).
    await this.#uploadTurn(options?.signal);
    try {
      return await uploadInParts(this.#http, this.workspaceOf({ containerId }), file, options);
    } finally {
      this.#uploading -= 1;
      this.#uploadWaiting.shift()?.();
    }
  }

  /** Uploads under way, and those waiting for their turn. */
  #uploading = 0;
  readonly #uploadWaiting: (() => void)[] = [];

  /**
   * Waits for a free place. One given up while waiting leaves the queue at
   * once, so that an upload ending wakes one that still wants its turn.
   */
  async #uploadTurn(signal: AbortSignal | undefined): Promise<void> {
    while (this.#uploading >= UPLOADS_AT_ONCE) {
      signal?.throwIfAborted();
      await new Promise<void>((resolve, reject) => {
        const giveUp = (): void => {
          const at = this.#uploadWaiting.indexOf(wake);
          if (at >= 0) this.#uploadWaiting.splice(at, 1);
          reject(signal?.reason as Error);
        };
        const wake = (): void => {
          signal?.removeEventListener('abort', giveUp);
          resolve();
        };
        this.#uploadWaiting.push(wake);
        signal?.addEventListener('abort', giveUp, { once: true });
      });
    }
    signal?.throwIfAborted();
    this.#uploading += 1;
  }

  /** `exportContainerArchive`, read with its progress. */
  exportContainer(id: string, options?: DownloadOptions): Promise<DownloadedFile> {
    return downloadWithProgress(
      this.#http,
      underWorkspace('/containers/{containerId}/archive/export'),
      this.workspaceOf({ containerId: id }),
      'archive.boardarchive',
      options,
    );
  }

  /** `exportWorkspaceArchive`, read with its progress. */
  exportWorkspace(workspaceId: string, options?: DownloadOptions): Promise<DownloadedFile> {
    return downloadWithProgress(
      this.#http,
      underWorkspace('/archive/export'),
      { workspaceId },
      'archive.boardarchive',
      options,
    );
  }

  /**
   * Imports an archive sent in parts (`upload.ts`). An archive over the
   * server's `maxImportBytes` is refused before anything is sent, with the
   * limit, as the server would (413). It takes a turn among the uploads: it
   * counts toward the same open uploads of the reader.
   */
  async importArchive(workspaceId: string, file: File, options?: UploadOptions) {
    const limit = (await this.#config.read()).maxImportBytes;
    if (file.size > limit) {
      throw tooLarge('archive', limit);
    }
    await this.#uploadTurn(options?.signal);
    try {
      return await importInParts(this.#http, workspaceId, file, options);
    } finally {
      this.#uploading -= 1;
      this.#uploadWaiting.shift()?.();
    }
  }

  /** The board's or the wiki's changes, from the one connection of the page (`live.ts`). */
  watchContainer(containerId: string, listener: ChangeListener): () => void {
    return this.#live.watch(containerId, listener);
  }

  /** The reader's own changes: boards and the sidebar. Settings have no message. */
  watchSelf(listener: ChangeListener): () => void {
    return this.#live.watch(undefined, listener);
  }

  /** The body in its room, on the page's one connection (its client is fetched then). */
  openDoc(workspaceId: string, _boardId: string, cardId: string, hears: BodyDocHears): BodyDoc {
    return openRoomLater(this.#live.rooms.line(workspaceId, cardId), hears);
  }

  /** The connection follows this workspace from now on, and no other. */
  override enter(workspaceId: string): void {
    super.enter(workspaceId);
    this.#live.enter(workspaceId);
  }
}
