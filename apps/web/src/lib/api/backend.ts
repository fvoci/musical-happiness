/**
 * The backend that calls the server: the data boundary
 * (`lib/state/backend.ts`) over the server's API (`contracts/openapi.json`).
 *
 * It is wired one feature at a time. What is not wired yet refuses with a
 * `BackendError` of the reason `not_wired` (see `notWired`), so that a
 * screen that reaches it says that the change was not saved instead of
 * failing quietly. What is not wired is what calls `notWired` below.
 */
import type { CardPreview } from '../editor/card-preview.ts';
import type { DocNode } from '../editor/schema.ts';
import type { BlockPatch, Board, BoardMember, BoardPatch, BoardView } from '../logic/wire.ts';
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
  type BoardRole,
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
  boardOf,
  cardDetailOf,
  cardOf,
  categoryOf,
  commentOf,
  commentVersionOf,
  memberOf,
  newBoardAndViewOf,
  newCardOf,
  boardPatchOf,
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
import { downloadWithProgress } from './download.ts';
import { Http } from './http.ts';
import { LiveChanges } from './live.ts';
import { ServerRequests } from './requests.ts';
import { seenUsers } from './seen-users.ts';
import { importInParts, uploadInParts } from './upload.ts';

/**
 * Refuses an operation that is not wired to the server yet. 405 is the
 * status the boundary has for "this cannot be done here"; the reason tells
 * it apart from a refusal of the server.
 */
export function notWired(operation: string): never {
  throw new BackendError(405, {
    detail: `${operation} is not wired to the server yet`,
    reason: 'not_wired',
  });
}

/** How many files go up at once; the server allows a reader 8 open uploads. */
const UPLOADS_AT_ONCE = 3;

export class ServerBackend extends ServerRequests implements Backend, Account {
  readonly #http: Http;
  /** The last change of each card or view on its way, settled either way (`#inTurn`). */
  readonly #writes = new Map<string, Promise<void>>();

  /** `basePath` is the path the app is served under (`<meta name="fvoci:base">`). */
  readonly #live: LiveChanges;

  constructor(basePath: string) {
    const http = new Http(basePath);
    super(http);
    this.#http = http;
    // The page's own origin: the browser sends the session's cookie with it.
    const socket = new URL(`${basePath}/ws`, location.href);
    socket.protocol = socket.protocol === 'https:' ? 'wss:' : 'ws:';
    this.#live = new LiveChanges(socket.href, {
      members: (boardId) => this.members(boardId),
    });
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

  /** The session goes on under a new cookie, which the browser keeps by itself. */
  async changePassword(oldPassword: string, newPassword: string): Promise<void> {
    const request: Schemas['ChangePasswordRequest'] = { oldPassword, newPassword };
    await this.#http.request('/api/v1/users/me/password', {}, { method: 'POST', json: request });
  }

  // ------------------------------------------- the reader and their workspace

  async me(): Promise<User> {
    return userOf(await this.#http.request<Schemas['User']>('/api/v1/users/me'));
  }

  /**
   * The people of the workspace. The server answers at most ten of them
   * (`getWorkspaceUsers` without a search; `docs/contract-assumptions.md`, D2).
   */
  async users(workspaceId: string): Promise<User[]> {
    const users = await this.#http.request<Schemas['User'][]>('/api/v1/w/{workspaceId}/users', {
      workspaceId,
    });
    return users.map(userOf);
  }

  async usersById(workspaceId: string, userIds: string[]): Promise<User[]> {
    const ask = (json: string[]) =>
      this.#http.request<Schemas['User'][]>(
        '/api/v1/w/{workspaceId}/users',
        { workspaceId },
        { method: 'POST', json },
      );
    return (await seenUsers([...new Set(userIds)], ask)).map(userOf);
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

  async boards(workspaceId: string): Promise<Board[]> {
    const boards = await this.#http.request<Schemas['Board'][]>('/api/v1/w/{workspaceId}/boards', {
      workspaceId,
    });
    return boards.map(boardOf);
  }

  async board(boardId: string): Promise<Board> {
    return boardOf(await this.inWorkspace<Schemas['Board']>('/boards/{boardId}', { boardId }));
  }

  /**
   * Makes the board and its first view in one request
   * (`createBoardsAndBlocks`): all or none. The identifiers sent only tie
   * the view to the board; the server makes the real ones.
   */
  async createBoard(
    workspaceId: string,
    draft: BoardDraft,
    firstView: ViewDraft,
  ): Promise<{ board: Board; view: BoardView }> {
    const request = newBoardAndViewOf(draft, firstView);
    const made = await this.inWorkspace<Schemas['BoardsAndBlocks']>(
      '/boards-and-blocks',
      {},
      { method: 'POST', json: request },
      workspaceId,
    );
    const [board] = made.boards;
    const view = made.blocks.find((block) => block.type === 'view');
    if (!board || !view) throw new BackendError(500, { detail: 'the server made no board' });
    return { board: boardOf(board), view: viewOf(view) };
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
  async patchBoard(boardId: string, patch: BoardPatch): Promise<Board> {
    const request = boardPatchOf(patch);
    return boardOf(
      await this.inWorkspace<Schemas['Board']>(
        '/boards/{boardId}',
        { boardId },
        { method: 'PATCH', json: request },
      ),
    );
  }

  /** `searchWorkspaceBoards` by title: the boards of the workspace. */
  async searchBoards(workspaceId: string, query: string): Promise<Board[]> {
    const boards = await this.#http.request<Schemas['Board'][]>(
      '/api/v1/w/{workspaceId}/boards/search',
      { workspaceId },
      { query: { q: query } },
    );
    return boards.map(boardOf);
  }

  async templates(workspaceId: string): Promise<Board[]> {
    const boards = await this.#http.request<Schemas['Board'][]>(
      '/api/v1/w/{workspaceId}/templates',
      { workspaceId },
    );
    return boards.map(boardOf);
  }

  /** `duplicateBoard` into the workspace (`toWorkspace`). The copy is the first board of the answer. */
  async duplicateBoard(boardId: string, asTemplate: boolean, workspaceId: string): Promise<Board> {
    const made = await this.inWorkspace<Schemas['BoardsAndBlocks']>(
      '/boards/{boardId}/duplicate',
      { boardId },
      { method: 'POST', query: { asTemplate: String(asTemplate), toWorkspace: workspaceId } },
    );
    const [board] = made.boards;
    if (!board) throw new BackendError(500, { detail: 'the server made no copy' });
    return boardOf(board);
  }

  async onboard(workspaceId: string): Promise<string> {
    const started = await this.#http.request<Schemas['OnboardingStart']>(
      '/api/v1/w/{workspaceId}/onboard',
      { workspaceId },
      { method: 'POST' },
    );
    return started.boardId;
  }

  /** To the server's trash, with its blocks and members (`deleteBoard`, board admins only). */
  async deleteBoard(boardId: string): Promise<void> {
    await this.inWorkspace('/boards/{boardId}', { boardId }, { method: 'DELETE' });
  }

  /** Out of the server's trash, filed where each person had it (the category messages tell). */
  async restoreBoard(boardId: string): Promise<Board> {
    return boardOf(
      await this.inWorkspace<Schemas['Board']>(
        '/boards/{boardId}/undelete',
        { boardId },
        { method: 'POST' },
      ),
    );
  }

  async members(boardId: string): Promise<BoardMember[]> {
    const members = await this.inWorkspace<Schemas['BoardMember'][]>('/boards/{boardId}/members', {
      boardId,
    });
    return members.map(memberOf);
  }

  /** `addMember` with exactly the one role, as the server gives roles. */
  async addMember(boardId: string, userId: string, role: BoardRole): Promise<BoardMember> {
    const request: Schemas['NewMember'] = { userId, ...rolesOf(role) };
    return memberOf(
      await this.inWorkspace<Schemas['BoardMember']>(
        '/boards/{boardId}/members',
        { boardId },
        { method: 'POST', json: request },
      ),
    );
  }

  async setMemberRole(boardId: string, userId: string, role: BoardRole): Promise<BoardMember> {
    return memberOf(
      await this.inWorkspace<Schemas['BoardMember']>(
        '/boards/{boardId}/members/{userId}',
        { boardId, userId },
        { method: 'PUT', json: rolesOf(role) },
      ),
    );
  }

  async removeMember(boardId: string, userId: string): Promise<void> {
    await this.inWorkspace(
      '/boards/{boardId}/members/{userId}',
      { boardId, userId },
      { method: 'DELETE' },
    );
  }

  async sharing(boardId: string): Promise<Sharing> {
    return sharingOf(
      await this.inWorkspace<Schemas['Sharing']>('/boards/{boardId}/sharing', {
        boardId,
      }),
    );
  }

  async setSharing(boardId: string, change: SharingChange): Promise<Sharing> {
    const request: Schemas['SharingRequest'] = {
      enabled: change.enabled,
      ...(change.regenerate ? { regenerate: true } : {}),
    };
    return sharingOf(
      await this.inWorkspace<Schemas['Sharing']>(
        '/boards/{boardId}/sharing',
        { boardId },
        { method: 'POST', json: request },
      ),
    );
  }

  /** `getWorkspaceUsers` with a search: ten at most, whatever the case (D2). */
  async searchUsers(workspaceId: string, query: string): Promise<User[]> {
    const users = await this.#http.request<Schemas['User'][]>(
      '/api/v1/w/{workspaceId}/users',
      { workspaceId },
      { query: { search: query } },
    );
    return users.map(userOf);
  }

  /** Joins an open board (`joinBoard`), with its minimum role. */
  async joinBoard(boardId: string): Promise<BoardMember> {
    return memberOf(
      await this.inWorkspace<Schemas['BoardMember']>(
        '/boards/{boardId}/join',
        { boardId },
        { method: 'POST' },
      ),
    );
  }

  // ------------------------------------------------------------ sidebar

  async categories(workspaceId: string): Promise<Category[]> {
    const categories = await this.#http.request<Schemas['CategoryBoards'][]>(
      '/api/v1/w/{workspaceId}/categories',
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
      '/api/v1/w/{workspaceId}/categories',
      { workspaceId },
      { method: 'POST', json: request },
    );
    return categoryOf({ ...made, boardMetadata: [] });
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
      '/api/v1/w/{workspaceId}/categories/{categoryId}',
      { workspaceId, categoryId },
      { method: 'PUT', json: request },
    );
    // The answer is the category alone; its boards are as they were read.
    return {
      ...categoryOf({ ...updated, boardMetadata: [] }),
      boardIds: before.boardIds,
      hiddenBoardIds: before.hiddenBoardIds,
    };
  }

  deleteCategory(workspaceId: string, categoryId: string): Promise<void> {
    return this.#sidebarTurn(workspaceId, async () => {
      await this.#http.request(
        '/api/v1/w/{workspaceId}/categories/{categoryId}',
        { workspaceId, categoryId },
        { method: 'DELETE' },
      );
    });
  }

  reorderCategories(workspaceId: string, categoryIds: string[]): Promise<void> {
    return this.#sidebarTurn(workspaceId, async () => {
      await this.#http.request(
        '/api/v1/w/{workspaceId}/category-order',
        { workspaceId },
        { method: 'PUT', json: categoryIds },
      );
    });
  }

  /**
   * Files a board under a category and puts it at its place: two requests,
   * since the server files a board (`setBoardCategory`) without a place and
   * orders a category's boards (`reorderCategoryBoards`) apart. The order is
   * made from the category as it is after the filing, read again.
   */
  moveBoardToCategory(
    workspaceId: string,
    boardId: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
    return this.#sidebarTurn(workspaceId, () =>
      this.#moveBoardToCategory(workspaceId, boardId, categoryId, index),
    );
  }

  async #moveBoardToCategory(
    workspaceId: string,
    boardId: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
    await this.#http.request(
      '/api/v1/w/{workspaceId}/categories/{categoryId}/boards/{boardId}',
      { workspaceId, categoryId, boardId },
      { method: 'POST' },
    );
    const filed = (await this.#category(workspaceId, categoryId)).boardIds.filter(
      (id) => id !== boardId,
    );
    filed.splice(Math.max(0, Math.min(index, filed.length)), 0, boardId);
    await this.#http.request(
      '/api/v1/w/{workspaceId}/categories/{categoryId}/board-order',
      { workspaceId, categoryId },
      { method: 'PUT', json: filed },
    );
  }

  setBoardHidden(
    workspaceId: string,
    categoryId: string,
    boardId: string,
    hidden: boolean,
  ): Promise<void> {
    return this.#sidebarTurn(workspaceId, async () => {
      await this.#http.request(
        hidden
          ? '/api/v1/w/{workspaceId}/categories/{categoryId}/boards/{boardId}/hide'
          : '/api/v1/w/{workspaceId}/categories/{categoryId}/boards/{boardId}/unhide',
        { workspaceId, categoryId, boardId },
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
      boardId,
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

  /** Every card of the board, in one page (`perPage` 0 asks for all). */
  async cards(boardId: string): Promise<CardSummary[]> {
    const cards = await this.inWorkspace<Schemas['Card'][]>(
      '/boards/{boardId}/cards',
      { boardId },
      { query: { perPage: 0 } },
    );
    return cards.map(cardOf);
  }

  /** The previews of the cards (`getCards` with `preview=true`); a card without one has none. */
  async cardPreviews(boardId: string): Promise<Map<string, CardPreview>> {
    const cards = await this.inWorkspace<Schemas['Card'][]>(
      '/boards/{boardId}/cards',
      { boardId },
      { query: { perPage: 0, preview: 'true' } },
    );
    return previewsOf(cards);
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
    const card = await this.inWorkspace<Schemas['Card']>('/cards/{cardId}', { cardId });
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
  patchCard(_boardId: string, cardId: string, patch: CardPatch): Promise<CardSummary> {
    return this.#inTurn(cardId, () => this.#patchCard(cardId, patch));
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

  async #patchCard(cardId: string, patch: CardPatch): Promise<CardSummary> {
    const request: Schemas['CardPatch'] = {};
    if (patch.title !== undefined) request.title = patch.title;
    if (patch.icon !== undefined) request.icon = patch.icon;
    const updated = patch.updatedProperties ?? {};
    if (Object.keys(updated).length > 0) request.updatedProperties = updated;
    const deleted = patch.deletedProperties ?? [];
    if (deleted.length > 0) request.deletedProperties = deleted;
    const card = await this.inWorkspace<Schemas['Card']>(
      '/cards/{cardId}',
      { cardId },
      { method: 'PATCH', json: request },
    );
    return cardOf(card);
  }

  /**
   * Replaces the body by the card patch, with the version it was read at:
   * the server refuses with 409 when the card changed since, and changes
   * nothing. An empty document is sent as it is; the server stores it as no
   * body. The body goes in the same turn as the card's other changes.
   */
  writeCardBody(_boardId: string, cardId: string, write: BodyWrite): Promise<CardDetail> {
    return this.#inTurn(cardId, async () => {
      const request: Schemas['CardPatch'] = {
        body: write.body ? wireBody(write.body) : null,
        readAt: write.readAt,
      };
      const card = await this.inWorkspace<Schemas['Card']>(
        '/cards/{cardId}',
        { cardId },
        { method: 'PATCH', json: request },
      );
      return cardDetailOf(card);
    });
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
      boardId,
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
      boardId,
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
   * (`getClientConfig`'s `maxFileSize`) is refused before anything is sent,
   * with the limit, as the server would refuse it (413).
   */
  async upload(boardId: string, file: File, options?: UploadOptions): Promise<UploadedFile> {
    const limit = (await this.#clientConfig()).maxFileSize;
    if (file.size > limit) {
      throw new BackendError(413, { detail: 'the file is too large', limit });
    }
    // A few at a time: the server keeps only so many open uploads of a reader (429).
    await this.#uploadTurn(options?.signal);
    try {
      return await uploadInParts(this.#http, this.workspaceOf({ boardId }), file, options);
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

  /** `exportBoardArchive`, read with its progress. */
  exportBoard(boardId: string, options?: DownloadOptions): Promise<DownloadedFile> {
    return downloadWithProgress(
      this.#http,
      '/api/v1/w/{workspaceId}/boards/{boardId}/archive/export',
      this.workspaceOf({ boardId }),
      'archive.boardarchive',
      options,
    );
  }

  /** `exportWorkspaceArchive`, read with its progress. */
  exportWorkspace(workspaceId: string, options?: DownloadOptions): Promise<DownloadedFile> {
    return downloadWithProgress(
      this.#http,
      '/api/v1/w/{workspaceId}/archive/export',
      { workspaceId },
      'archive.boardarchive',
      options,
    );
  }

  /**
   * Imports an archive sent in parts (`upload.ts`). An archive over the
   * server's `maxImportSize` is refused before anything is sent, with the
   * limit, as the server would (413). It takes a turn among the uploads: it
   * counts toward the same open uploads of the reader.
   */
  async importArchive(workspaceId: string, file: File, options?: UploadOptions) {
    const limit = (await this.#clientConfig()).maxImportSize;
    if (file.size > limit) {
      // The reason is this app's: the server's 413 may count entries, this one counts bytes.
      throw new BackendError(413, {
        detail: 'the archive is too large',
        reason: 'over_import_limit',
        limit,
      });
    }
    await this.#uploadTurn(options?.signal);
    try {
      return await importInParts(this.#http, workspaceId, file, options);
    } finally {
      this.#uploading -= 1;
      this.#uploadWaiting.shift()?.();
    }
  }

  /** The server's settings for clients, read once. */
  #config: Promise<Schemas['ClientConfig']> | undefined;

  #clientConfig(): Promise<Schemas['ClientConfig']> {
    this.#config ??= this.#http.request<Schemas['ClientConfig']>('/api/v1/client-config');
    // A failed reading is tried again next time.
    this.#config.catch(() => {
      this.#config = undefined;
    });
    return this.#config;
  }

  /** The bytes of a file (`getFile`). */
  fileBytes(boardId: string, fileId: string): Promise<Blob> {
    return this.inWorkspaceBytes('/boards/{boardId}/files/{fileId}', { boardId, fileId });
  }

  /** The file's bytes, from the page's own origin: the session's cookie goes along. */
  fileUrl(boardId: string, fileId: string): string | undefined {
    return this.inWorkspaceUrl('/boards/{boardId}/files/{fileId}', { boardId, fileId });
  }

  // ------------------------------------------------------------ changes

  /** The board's changes, from the one connection of the page (`live.ts`). */
  watchBoard(boardId: string, listener: ChangeListener): () => void {
    return this.#live.watch(boardId, listener);
  }

  /** The reader's own changes: boards and the sidebar. Settings have no message. */
  watchSelf(listener: ChangeListener): () => void {
    return this.#live.watch(undefined, listener);
  }

  /** The connection follows this workspace from now on, and no other. */
  override enter(workspaceId: string): void {
    super.enter(workspaceId);
    this.#live.enter(workspaceId);
  }
}
