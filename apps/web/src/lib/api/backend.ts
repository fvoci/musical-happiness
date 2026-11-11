/**
 * The backend that calls the server: the data boundary
 * (`lib/state/backend.ts`) over the server's API (`contracts/openapi.json`).
 *
 * It is wired one feature at a time. What is not wired yet refuses with a
 * `BackendError` of the reason `not_wired` (see `notWired`), so that a
 * screen that reaches it says that the change was not saved instead of
 * failing quietly. Which operations are wired is in `docs/handoff.md`.
 */
import type { DocNode } from '../editor/schema.ts';
import type { BlockPatch, Board, BoardMember, BoardPatch, BoardView } from '../logic/wire.ts';
import { applyViewPatch } from '../state/patches.ts';
import {
  BackendError,
  type Account,
  type AccountSettings,
  type AccountSettingsPatch,
  type Attachment,
  type Backend,
  type BoardDraft,
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
  type Registration,
  type Team,
  type UploadedFile,
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
  memberOf,
  newCardOf,
  settingsChangeOf,
  settingsOf,
  teamOf,
  userOf,
  viewOf,
  wireBody,
  type Schemas,
} from './convert.ts';
import { Http } from './http.ts';
import { LiveChanges } from './live.ts';

/**
 * Refuses an operation that is not wired to the server yet. 405 is the
 * status the boundary has for "this cannot be done here"; the reason tells
 * it apart from a refusal of the server.
 */
export function notWired(operation: string): never {
  throw new BackendError(405, {
    message: `${operation} is not wired to the server yet`,
    reason: 'not_wired',
  });
}

export class ServerBackend implements Backend, Account {
  readonly #http: Http;
  /** The last change of each card or view on its way, settled either way (`#inTurn`). */
  readonly #writes = new Map<string, Promise<void>>();

  /** `basePath` is the path the app is served under (`<meta name="fvoci:base">`). */
  readonly #live: LiveChanges;

  constructor(basePath: string) {
    this.#http = new Http(basePath);
    // The page's own origin: the browser sends the session's cookie with it.
    const socket = new URL(`${basePath}/ws`, location.href);
    socket.protocol = socket.protocol === 'https:' ? 'wss:' : 'ws:';
    this.#live = new LiveChanges(socket.href, {
      teamId: async () => {
        const [team] = await this.teams();
        if (!team) throw new Error('the reader belongs to no team');
        return team.id;
      },
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

  async register(registration: Registration): Promise<void> {
    const request: Schemas['RegisterRequest'] = {
      username: registration.username,
      email: registration.email,
      password: registration.password,
      ...(registration.token ? { token: registration.token } : {}),
    };
    await this.#http.request('/api/v1/register', {}, { method: 'POST', json: request });
  }

  async signOut(): Promise<void> {
    await this.#http.request('/api/v1/logout', {}, { method: 'POST' });
  }

  // ------------------------------------------- the reader and their team

  async me(): Promise<User> {
    return userOf(await this.#http.request<Schemas['User']>('/api/v1/users/me'));
  }

  async teams(): Promise<Team[]> {
    return (await this.#http.request<Schemas['Team'][]>('/api/v1/teams')).map(teamOf);
  }

  /**
   * The people of the team. The server answers at most ten of them
   * (`getTeamUsers` without a search; `docs/contract-assumptions.md`, D2).
   */
  async users(teamId: string): Promise<User[]> {
    const users = await this.#http.request<Schemas['User'][]>('/api/v1/teams/{teamId}/users', {
      teamId,
    });
    return users.map(userOf);
  }

  async usersById(teamId: string, userIds: string[]): Promise<User[]> {
    const ids = [...new Set(userIds)];
    if (ids.length === 0) return [];
    const users = await this.#http.request<Schemas['User'][]>(
      '/api/v1/teams/{teamId}/users',
      { teamId },
      { method: 'POST', json: ids },
    );
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

  async boards(teamId: string): Promise<Board[]> {
    const boards = await this.#http.request<Schemas['Board'][]>('/api/v1/teams/{teamId}/boards', {
      teamId,
    });
    return boards.map(boardOf);
  }

  async board(boardId: string): Promise<Board> {
    return boardOf(
      await this.#http.request<Schemas['Board']>('/api/v1/boards/{boardId}', { boardId }),
    );
  }

  createBoard(_teamId: string, _draft: BoardDraft): Promise<Board> {
    return notWired('createBoard');
  }

  patchBoard(_boardId: string, _patch: BoardPatch): Promise<Board> {
    return notWired('patchBoard');
  }

  deleteBoard(_boardId: string): Promise<void> {
    return notWired('deleteBoard');
  }

  restoreBoard(_boardId: string): Promise<Board> {
    return notWired('restoreBoard');
  }

  async members(boardId: string): Promise<BoardMember[]> {
    const members = await this.#http.request<Schemas['BoardMember'][]>(
      '/api/v1/boards/{boardId}/members',
      { boardId },
    );
    return members.map(memberOf);
  }

  // ------------------------------------------------------------ sidebar

  async categories(teamId: string): Promise<Category[]> {
    const categories = await this.#http.request<Schemas['CategoryBoards'][]>(
      '/api/v1/teams/{teamId}/categories',
      { teamId },
    );
    return categories.map(categoryOf);
  }

  createCategory(_teamId: string, _draft: CategoryDraft): Promise<Category> {
    return notWired('createCategory');
  }

  patchCategory(_categoryId: string, _patch: CategoryPatch): Promise<Category> {
    return notWired('patchCategory');
  }

  deleteCategory(_categoryId: string): Promise<void> {
    return notWired('deleteCategory');
  }

  reorderCategories(_teamId: string, _categoryIds: string[]): Promise<void> {
    return notWired('reorderCategories');
  }

  moveBoardToCategory(_boardId: string, _categoryId: string, _index: number): Promise<void> {
    return notWired('moveBoardToCategory');
  }

  // ------------------------------------------------------- views, cards

  async views(boardId: string): Promise<BoardView[]> {
    const blocks = await this.#http.request<Schemas['Block'][]>(
      '/api/v1/boards/{boardId}/blocks',
      { boardId },
      { query: { type: 'view' } },
    );
    return blocks.map(viewOf);
  }

  createView(_boardId: string, _draft: ViewDraft): Promise<BoardView> {
    return notWired('createView');
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
    await this.#http.request(
      '/api/v1/boards/{boardId}/blocks/{blockId}',
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
    const [view] = await this.#http.request<Schemas['Block'][]>(
      '/api/v1/boards/{boardId}/blocks',
      { boardId },
      { query: { blockId: viewId } },
    );
    if (view?.type !== 'view') {
      throw new BackendError(404, { message: 'no such view on this board', reason: 'not_found' });
    }
    return viewOf(view);
  }

  /** Every card of the board, in one page (`perPage` 0 asks for all). */
  async cards(boardId: string): Promise<CardSummary[]> {
    const cards = await this.#http.request<Schemas['Card'][]>(
      '/api/v1/boards/{boardId}/cards',
      { boardId },
      { query: { perPage: 0 } },
    );
    return cards.map(cardOf);
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
    const card = await this.#http.request<Schemas['Card']>('/api/v1/cards/{cardId}', { cardId });
    if (card.boardId !== boardId) {
      throw new BackendError(404, { message: 'no such card on this board', reason: 'not_found' });
    }
    return card;
  }

  async createCard(boardId: string, draft: CardDraft): Promise<CardSummary> {
    const card = await this.#http.request<Schemas['Card']>(
      '/api/v1/boards/{boardId}/cards',
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
    const card = await this.#http.request<Schemas['Card']>(
      '/api/v1/cards/{cardId}',
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
      const card = await this.#http.request<Schemas['Card']>(
        '/api/v1/cards/{cardId}',
        { cardId },
        { method: 'PATCH', json: request },
      );
      return cardDetailOf(card);
    });
  }

  /** The comments of a card, read only for now (`commentOf`). */
  async comments(boardId: string, cardId: string): Promise<Comment[]> {
    return (await this.#children(boardId, cardId, 'comment')).map(commentOf);
  }

  createComment(_boardId: string, _cardId: string, _body: DocNode): Promise<Comment> {
    return notWired('createComment');
  }

  /** The attachments of a card, read only for now (`attachmentOf`). */
  async attachments(boardId: string, cardId: string): Promise<Attachment[]> {
    return (await this.#children(boardId, cardId, 'attachment')).map(attachmentOf);
  }

  /** The blocks of one type right under a block. */
  async #children(boardId: string, parentId: string, type: string): Promise<Schemas['Block'][]> {
    return this.#http.request<Schemas['Block'][]>(
      '/api/v1/boards/{boardId}/blocks',
      { boardId },
      { query: { parentId, type } },
    );
  }

  createAttachment(_boardId: string, _cardId: string, _file: UploadedFile): Promise<Attachment> {
    return notWired('createAttachment');
  }

  /** To the server's trash, with what is under it (204). */
  async deleteBlock(boardId: string, blockId: string): Promise<void> {
    await this.#http.request(
      '/api/v1/boards/{boardId}/blocks/{blockId}',
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
    await this.#http.request(
      '/api/v1/boards/{boardId}/blocks/{blockId}/undelete',
      { boardId, blockId },
      { method: 'POST' },
    );
  }

  upload(_boardId: string, _file: File): Promise<UploadedFile> {
    return notWired('upload');
  }

  fileUrl(_boardId: string, _fileId: string): string | undefined {
    return undefined;
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
}
