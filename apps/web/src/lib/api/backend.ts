/**
 * The backend that calls the server: the data boundary
 * (`lib/state/backend.ts`) over the server's API (`contracts/openapi.json`).
 *
 * It is wired one feature at a time. What is not wired yet refuses with a
 * `BackendError` of the reason `not_wired` (see `notWired`), so that a
 * screen that reaches it says that the change was not saved instead of
 * failing quietly. What is not wired is what calls `notWired` below.
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
import { uploadInParts } from './upload.ts';

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

/** How many files go up at once; the server allows a reader 8 open uploads. */
const UPLOADS_AT_ONCE = 3;

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

  /** Joins an open board (`joinBoard`), with its minimum role. */
  async joinBoard(boardId: string): Promise<BoardMember> {
    return memberOf(
      await this.#http.request<Schemas['BoardMember']>(
        '/api/v1/boards/{boardId}/join',
        { boardId },
        { method: 'POST' },
      ),
    );
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
    const [made] = await this.#http.request<Schemas['Block'][]>(
      '/api/v1/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { message: 'the server made no view' });
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
    const [made] = await this.#http.request<Schemas['Block'][]>(
      '/api/v1/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { message: 'the server made no comment' });
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
        new BackendError(400, { message: 'a comment needs a text', field: 'body' }),
      );
    }
    return this.#inTurn(commentId, async () => {
      const before = await this.#comment(boardId, commentId);
      const patch: Schemas['BlockPatch'] = {
        body: write.body ? wireBody(write.body) : null,
        readAt: write.readAt,
      };
      await this.#http.request(
        '/api/v1/boards/{boardId}/blocks/{blockId}',
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

  /** A comment of this board, by its block: 404 for another block or none. */
  async #comment(boardId: string, commentId: string): Promise<Comment> {
    const [block] = await this.#http.request<Schemas['Block'][]>(
      '/api/v1/boards/{boardId}/blocks',
      { boardId },
      { query: { blockId: commentId } },
    );
    if (block?.type !== 'comment') {
      throw new BackendError(404, {
        message: 'no such comment on this board',
        reason: 'not_found',
      });
    }
    return commentOf(block);
  }

  /** The attachments of a card (`attachmentOf`). */
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
    const [made] = await this.#http.request<Schemas['Block'][]>(
      '/api/v1/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { message: 'the server made no attachment' });
    return attachmentOf(made);
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

  /**
   * Uploads a file in parts (`upload.ts`). A file over the server's limit
   * (`getClientConfig`'s `maxFileSize`) is refused before anything is sent,
   * with the limit, as the server would refuse it (413).
   */
  async upload(boardId: string, file: File, options?: UploadOptions): Promise<UploadedFile> {
    const limit = (await this.#clientConfig()).maxFileSize;
    if (file.size > limit) {
      throw new BackendError(413, { message: 'the file is too large', limit });
    }
    // A few at a time: the server keeps only so many open uploads of a reader (429).
    await this.#uploadTurn(options?.signal);
    try {
      return await uploadInParts(this.#http, boardId, file, options);
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
    return this.#http.bytes('/api/v1/boards/{boardId}/files/{fileId}', { boardId, fileId });
  }

  /** The file's bytes, from the page's own origin: the session's cookie goes along. */
  fileUrl(boardId: string, fileId: string): string | undefined {
    return this.#http.url('/api/v1/boards/{boardId}/files/{fileId}', { boardId, fileId });
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
