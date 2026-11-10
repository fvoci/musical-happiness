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
  boardOf,
  categoryOf,
  settingsChangeOf,
  settingsOf,
  teamOf,
  userOf,
  type Schemas,
} from './convert.ts';
import { Http } from './http.ts';

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

  /** `basePath` is the path the app is served under (`<meta name="fvoci:base">`). */
  constructor(basePath: string) {
    this.#http = new Http(basePath);
  }

  // ------------------------------------------------------------ account

  /**
   * The server takes the name first and the email address when no user has
   * that name; a name may have an `@` in it. So the text goes as both.
   */
  async signIn(login: string, password: string): Promise<void> {
    const request: Schemas['LoginRequest'] = { username: login, email: login, password };
    await this.#http.request('/api/v1/login', {}, { method: 'POST', json: request });
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

  board(_boardId: string): Promise<Board> {
    return notWired('board');
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

  members(_boardId: string): Promise<BoardMember[]> {
    return notWired('members');
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

  views(_boardId: string): Promise<BoardView[]> {
    return notWired('views');
  }

  createView(_boardId: string, _draft: ViewDraft): Promise<BoardView> {
    return notWired('createView');
  }

  patchView(_boardId: string, _viewId: string, _patch: BlockPatch): Promise<BoardView> {
    return notWired('patchView');
  }

  cards(_boardId: string): Promise<CardSummary[]> {
    return notWired('cards');
  }

  card(_boardId: string, _cardId: string): Promise<CardDetail> {
    return notWired('card');
  }

  createCard(_boardId: string, _draft: CardDraft): Promise<CardSummary> {
    return notWired('createCard');
  }

  patchCard(_boardId: string, _cardId: string, _patch: CardPatch): Promise<CardSummary> {
    return notWired('patchCard');
  }

  /** Bodies are not wired: the server is changing how it stores them (`docs/contract-pending.md`, 1). */
  writeCardBody(_boardId: string, _cardId: string, _write: BodyWrite): Promise<CardDetail> {
    return notWired('writeCardBody');
  }

  comments(_boardId: string, _cardId: string): Promise<Comment[]> {
    return notWired('comments');
  }

  createComment(_boardId: string, _cardId: string, _body: DocNode): Promise<Comment> {
    return notWired('createComment');
  }

  attachments(_boardId: string, _cardId: string): Promise<Attachment[]> {
    return notWired('attachments');
  }

  createAttachment(_boardId: string, _cardId: string, _file: UploadedFile): Promise<Attachment> {
    return notWired('createAttachment');
  }

  deleteBlock(_boardId: string, _blockId: string): Promise<void> {
    return notWired('deleteBlock');
  }

  restoreBlock(_boardId: string, _blockId: string): Promise<void> {
    return notWired('restoreBlock');
  }

  upload(_boardId: string, _file: File): Promise<UploadedFile> {
    return notWired('upload');
  }

  fileUrl(_boardId: string, _fileId: string): string | undefined {
    return undefined;
  }

  // ------------------------------------------------------------ changes

  watchBoard(_boardId: string, _listener: ChangeListener): () => void {
    return () => undefined;
  }

  watchSelf(_listener: ChangeListener): () => void {
    return () => undefined;
  }
}
