/**
 * The backend of a reader by a board's public link: the board, its views,
 * cards, comments and files, read by `/api/v1/shared/{boardId}` with the
 * link's read token alone. No session goes along (`credentials: 'omit'`): a
 * reader who is signed in reads as anyone else does, and nothing of their
 * account mixes in. Nothing is written: every change is refused here,
 * before a request is made. A visit connection counts this reader anonymously;
 * content is read again to see what is new, and no presence roster is received.
 *
 * The board is the only one of a place of its own (`SHARED_PLACE`), with
 * nobody in it: the server names no person to such a reader.
 */
import { SharedPresence } from './shared-presence.ts';
import type { BoardView } from '../logic/wire.ts';
import { DEFAULT_ACCOUNT_SETTINGS } from '../logic/account-settings.ts';
import {
  BackendError,
  type AccountSettings,
  type Attachment,
  type Backend,
  type CardDetail,
  type CardSummary,
  type Comment,
  type User,
  type WorkspaceInfo,
} from '../state/backend.ts';
import type { CardPreview } from '../editor/card-preview.ts';
import type { Board } from '../logic/wire.ts';

import {
  attachmentOf,
  boardOf,
  bodyOf,
  cardOf,
  cardOfBlock,
  commentOf,
  previewsOf,
  viewOf,
  type Schemas,
} from './convert.ts';
import { Http, type ApiPath } from './http.ts';

/** The place the shared board is shown in: no workspace of anybody's. */
export const SHARED_PLACE = 'shared';

/** The reader by the link: nobody. */
const READER: User = {
  id: '',
  username: '',
  displayName: '',
  email: '',
  createAt: 0,
  updateAt: 0,
  deleteAt: 0,
};

/** A change, which a reader by the link does not make. */
function refused(): Promise<never> {
  return Promise.reject(
    new BackendError(403, { detail: 'a reader by the link reads only', reason: 'read_only' }),
  );
}

const nothing = (): Promise<never[]> => Promise.resolve([]);
const done = (): Promise<void> => Promise.resolve();
const unwatched = (): (() => void) => () => undefined;

/** A path of the shared board's, as the contract writes it after `/api/v1/shared/{boardId}`. */
type SharedPath = ApiPath extends infer P
  ? P extends `/api/v1/shared/{boardId}${infer Rest}`
    ? Rest
    : never
  : never;

export class SharedBackend implements Backend {
  readonly presence: Backend['presence'];
  readonly #http: Http;
  readonly #boardId: string;
  readonly #readToken: string;
  readonly #settings: AccountSettings;

  /**
   * `basePath` as `ServerBackend` takes it; the board and the token are the
   * link's. `settings` are the device's (its theme and density), which the
   * reader keeps: nobody's account has any here.
   */
  constructor(
    basePath: string,
    boardId: string,
    readToken: string,
    settings: AccountSettings = DEFAULT_ACCOUNT_SETTINGS,
  ) {
    this.#http = new Http(basePath, 'omit');
    this.presence = new SharedPresence(basePath, boardId, readToken);
    this.#boardId = boardId;
    this.#readToken = readToken;
    this.#settings = settings;
  }

  /** Every address of the reader's is made here: the board's, with the token. */
  #read<T>(
    path: SharedPath,
    params: Record<string, string> = {},
    query: Record<string, string | number> = {},
  ): Promise<T> {
    return this.#http.request<T>(
      `/api/v1/shared/{boardId}${path}`,
      { boardId: this.#boardId, ...params },
      { query: { ...query, readToken: this.#readToken } },
    );
  }

  /** Only the link's board is there: another is as one that does not exist. */
  #only(boardId: string): void {
    if (boardId !== this.#boardId) throw new BackendError(404, { detail: 'board not found' });
  }

  me(): Promise<User> {
    return Promise.resolve(READER);
  }

  workspaces(): Promise<WorkspaceInfo[]> {
    return Promise.resolve([{ id: SHARED_PLACE, title: '' }]);
  }

  settings(): Promise<AccountSettings> {
    return Promise.resolve({ ...this.#settings });
  }

  screenState(): Promise<Record<string, string>> {
    return Promise.resolve({});
  }

  async board(boardId: string): Promise<Board> {
    this.#only(boardId);
    const board = boardOf(await this.#read<Schemas['Board']>(''));
    return { ...board, workspaceId: SHARED_PLACE };
  }

  async views(boardId: string): Promise<BoardView[]> {
    this.#only(boardId);
    const blocks = await this.#read<Schemas['Block'][]>('/blocks', {}, { type: 'view' });
    return blocks.map(viewOf);
  }

  async cards(boardId: string): Promise<CardSummary[]> {
    this.#only(boardId);
    const cards = await this.#read<Schemas['Card'][]>('/cards', {}, { perPage: 0 });
    return cards.map(cardOf);
  }

  async cardPreviews(boardId: string): Promise<Map<string, CardPreview>> {
    this.#only(boardId);
    const cards = await this.#read<Schemas['Card'][]>(
      '/cards',
      {},
      { perPage: 0, preview: 'true' },
    );
    return previewsOf(cards);
  }

  /** A card with its body: its block, which carries the body a list of cards does not. */
  async card(boardId: string, cardId: string): Promise<CardDetail> {
    this.#only(boardId);
    const [block] = await this.#read<Schemas['Block'][]>(
      '/blocks',
      {},
      { blockId: cardId, type: 'card' },
    );
    // A block of another kind in the place of a card is no card.
    if (block?.type !== 'card') throw new BackendError(404, { detail: 'card not found' });
    return { ...cardOfBlock(block), body: bodyOf(block.body) };
  }

  async comments(boardId: string, cardId: string): Promise<Comment[]> {
    return (await this.#children(boardId, cardId, 'comment')).map(commentOf);
  }

  async attachments(boardId: string, cardId: string): Promise<Attachment[]> {
    return (await this.#children(boardId, cardId, 'attachment')).map(attachmentOf);
  }

  #children(boardId: string, parentId: string, type: string): Promise<Schemas['Block'][]> {
    this.#only(boardId);
    return this.#read<Schemas['Block'][]>('/blocks', {}, { parentId, type });
  }

  fileBytes(boardId: string, fileId: string): Promise<Blob> {
    this.#only(boardId);
    return this.#http.bytes(
      '/api/v1/shared/{boardId}/files/{fileId}',
      { boardId, fileId },
      { readToken: this.#readToken },
    );
  }

  fileUrl(boardId: string, fileId: string): string | undefined {
    if (boardId !== this.#boardId) return undefined;
    return this.#http.url(
      '/api/v1/shared/{boardId}/files/{fileId}',
      { boardId, fileId },
      { readToken: this.#readToken },
    );
  }

  // Nobody is named to the reader, and the place has no sidebar.
  users: Backend['users'] = nothing;
  usersById: Backend['usersById'] = nothing;
  members: Backend['members'] = nothing;
  boards: Backend['boards'] = nothing;
  categories: Backend['categories'] = nothing;
  templates: Backend['templates'] = nothing;
  searchBoards: Backend['searchBoards'] = nothing;
  searchUsers: Backend['searchUsers'] = nothing;
  commentVersions: Backend['commentVersions'] = nothing;
  deletedBoards: Backend['deletedBoards'] = nothing;
  deletedCards: Backend['deletedCards'] = nothing;

  // What is on the screen alone: kept for the visit.
  patchScreenState: Backend['patchScreenState'] = done;

  // Nothing changes while the board is read.
  watchBoard: Backend['watchBoard'] = unwatched;
  watchSelf: Backend['watchSelf'] = unwatched;

  // Every change is refused before it is sent.
  patchMe: Backend['patchMe'] = refused;
  patchSettings: Backend['patchSettings'] = refused;
  createBoard: Backend['createBoard'] = refused;
  patchBoard: Backend['patchBoard'] = refused;
  reorderCardProperties: Backend['reorderCardProperties'] = refused;
  duplicateBoard: Backend['duplicateBoard'] = refused;
  onboard: Backend['onboard'] = refused;
  deleteBoard: Backend['deleteBoard'] = refused;
  restoreBoard: Backend['restoreBoard'] = refused;
  addMember: Backend['addMember'] = refused;
  setMemberRole: Backend['setMemberRole'] = refused;
  removeMember: Backend['removeMember'] = refused;
  sharing: Backend['sharing'] = refused;
  setSharing: Backend['setSharing'] = refused;
  joinBoard: Backend['joinBoard'] = refused;
  createCategory: Backend['createCategory'] = refused;
  patchCategory: Backend['patchCategory'] = refused;
  deleteCategory: Backend['deleteCategory'] = refused;
  reorderCategories: Backend['reorderCategories'] = refused;
  moveBoardToCategory: Backend['moveBoardToCategory'] = refused;
  setBoardHidden: Backend['setBoardHidden'] = refused;
  createView: Backend['createView'] = refused;
  patchView: Backend['patchView'] = refused;
  createCard: Backend['createCard'] = refused;
  patchCard: Backend['patchCard'] = refused;
  createComment: Backend['createComment'] = refused;
  patchComment: Backend['patchComment'] = refused;
  createAttachment: Backend['createAttachment'] = refused;
  deleteBlock: Backend['deleteBlock'] = refused;
  restoreBlock: Backend['restoreBlock'] = refused;
  upload: Backend['upload'] = refused;
  exportBoard: Backend['exportBoard'] = refused;
  exportWorkspace: Backend['exportWorkspace'] = refused;
  importArchive: Backend['importArchive'] = refused;
}
