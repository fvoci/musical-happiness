/**
 * The data boundary: everything a screen can read, write and listen to.
 *
 * Screens never talk to a server. They use the stores of this folder, and
 * the stores use one `Backend`: the one that calls the server (`lib/api/`),
 * or the one that keeps its data in memory (`memory/`) for working without a
 * server. Nothing above this file knows which.
 *
 * The shapes follow the server design (its `docs/s6-design.md`): errors are
 * the problem details of RFC 9457 with the status codes of its section 1.3, identifiers are made by the server, a card or a comment has
 * its text as one document (`body`), a card list comes without bodies, and
 * a body is written with the `updateAt` it was read at so that a stale write
 * is refused with 409. Where the design says nothing and something had to be
 * made up and is still open, it is listed in `docs/contract-assumptions.md`.
 *
 * No method promises an order. Whatever is shown in an order is sorted by
 * the client (`order.ts`).
 */
import type { DocNode } from '../editor/schema.ts';
import type { DisplaySettings } from '../logic/display.ts';
import type { RegionalSettings } from '../logic/regional.ts';
import type { UserNames } from '../logic/users.ts';
import type {
  Block,
  BlockPatch,
  Board,
  BoardMember,
  BoardPatch,
  BoardView,
  Card,
} from '../logic/wire.ts';

// ----------------------------------------------------------------- errors

/** The statuses the server answers an error with (server design 1.3). */
export const ERROR_STATUSES = [400, 401, 403, 404, 405, 409, 412, 413, 416, 429, 500] as const;

export type ErrorStatus = (typeof ERROR_STATUSES)[number];

/**
 * One thing of a request that was refused, and where it is: exactly one of
 * `pointer` (a JSON Pointer into the body, `/username`), `parameter` (of the
 * query or the path) and `header` says.
 */
export interface InvalidInput {
  pointer?: string;
  parameter?: string;
  header?: string;
  /** English, for logs and developers. Never shown as it is. */
  detail: string;
  /** Why, stable: `required`, `too_short`, …, or `invalid` when no finer cause is told. */
  reason: string;
}

/** The body of an error answer: the problem details of RFC 9457, as the contract has them (`Problem`). */
export interface Problem {
  /** The kind of problem, `/problems/<name>`: the same for every problem of the kind. */
  type: string;
  /** The name of the type, in English. */
  title: string;
  status: number;
  /** English, for logs and developers. Never shown as it is. */
  detail: string;
  /** The request, as a URN of its identifier. */
  instance?: string;
  /** A stable reason within the status, for choosing the text to show. */
  reason?: string;
  /** What of the request was refused, one entry for each. */
  errors?: InvalidInput[];
  limit?: number;
  requestId?: string;
  /** Seconds, with 429. */
  retryAfter?: number;
}

/** The problems of the statuses as the server names them: the last part of `type`, and the title. */
const STATUS_PROBLEMS: Record<ErrorStatus, readonly [name: string, title: string]> = {
  400: ['bad-request', 'Bad request'],
  401: ['unauthorized', 'No valid session'],
  403: ['forbidden', 'Forbidden'],
  404: ['not-found', 'Not found'],
  405: ['method-not-allowed', 'Method not allowed'],
  409: ['conflict', 'Conflict'],
  412: ['precondition-failed', 'Precondition failed'],
  413: ['payload-too-large', 'Payload too large'],
  416: ['range-not-satisfiable', 'Range not satisfiable'],
  429: ['rate-limited', 'Too many requests'],
  500: ['internal', 'Internal server error'],
};

/** The problem the server answers with for a status that has no `reason`. */
export function problemOf(status: ErrorStatus, detail: string): Problem {
  const [name, title] = STATUS_PROBLEMS[status];
  return { type: `/problems/${name}`, title, status, detail };
}

/** The entry of `errors` for a member of the request body, by its name. */
export function refusedMember(member: string, detail: string, reason = 'invalid'): InvalidInput {
  return { pointer: `/${member.replaceAll('~', '~0').replaceAll('/', '~1')}`, detail, reason };
}

/** What every method of the backend rejects with. */
export class BackendError extends Error {
  /** 0 when no answer arrived at all. */
  readonly status: ErrorStatus | 0;
  /** The kind of problem; `about:blank` when the answer named none (RFC 9457, 4.2). */
  readonly type: string;
  readonly reason: string | undefined;
  /** What of the request was refused; empty when the error is about nothing of it. */
  readonly errors: readonly InvalidInput[];
  readonly limit: number | undefined;
  readonly retryAfter: number | undefined;
  /** The server's name for the request, to report a failure by. */
  readonly requestId: string | undefined;

  constructor(status: ErrorStatus | 0, problem?: Partial<Problem>) {
    super(problem?.detail ?? problem?.title ?? (status === 0 ? 'no answer' : `status ${status}`));
    this.name = 'BackendError';
    this.status = status;
    this.type = problem?.type ?? 'about:blank';
    this.reason = problem?.reason;
    this.errors = problem?.errors ?? [];
    this.limit = problem?.limit;
    this.retryAfter = problem?.retryAfter;
    this.requestId = problem?.requestId;
  }

  /** The entry of `errors` about the member of the request body at this JSON Pointer, if there is one. */
  refusalAt(pointer: string): InvalidInput | undefined {
    return this.errors.find((entry) => entry.pointer === pointer);
  }
}

export function isBackendError(error: unknown): error is BackendError {
  return error instanceof BackendError;
}

/** The stored version differs from the one the write was based on. */
export function isConflict(error: unknown): error is BackendError {
  return isBackendError(error) && error.status === 409;
}

// ------------------------------------------------------------------ model

export interface User extends UserNames {
  id: string;
  email: string;
  /** Only on the reader's own user. */
  isAdmin?: boolean;
  createAt: number;
  updateAt: number;
  /** Not 0: the account was deactivated; it stays so that what it made keeps an author. */
  deleteAt: number;
}

export interface Team {
  id: string;
  title: string;
}

/**
 * The reader's account settings: the three of the server design (5.8) and
 * the four display settings the plan adds to them. The answer always holds
 * every member.
 */
export type AccountSettings = RegionalSettings & DisplaySettings;

/** A member set to `null` goes back to its default. */
export type AccountSettingsPatch = { [K in keyof AccountSettings]?: AccountSettings[K] | null };

/** A category of the reader's sidebar, with the boards filed under it. */
export interface Category {
  id: string;
  name: string;
  userId: string;
  teamId: string;
  /** `system` is the one every reader has and cannot delete. */
  type: 'custom' | 'system';
  collapsed: boolean;
  /** The reader's order of the categories; ties are the client's to break. */
  sortOrder: number;
  /** The boards in the reader's order. */
  boardIds: string[];
  /** Boards of this category the reader hid from the sidebar. */
  hiddenBoardIds: string[];
  createAt: number;
  updateAt: number;
}

/** A role on a board: one of them, the highest a member has. */
export type BoardRole = 'admin' | 'editor' | 'commenter' | 'viewer';

/**
 * A board's public link: whether it is on, and the token a reader without
 * an account reads the board with (empty while it is off and was never on).
 */
export interface Sharing {
  enabled: boolean;
  token: string;
}

export interface SharingChange {
  enabled: boolean;
  /** Makes a new token: the old link stops working. */
  regenerate?: boolean;
}

export interface CategoryDraft {
  name: string;
}

export interface CategoryPatch {
  name?: string;
  collapsed?: boolean;
}

/** A card as a list has it: no body, but whether there is one and how its tasks stand. */
export interface CardSummary extends Card {
  hasBody: boolean;
  tasksTotal: number;
  tasksDone: number;
}

/** A card read on its own: with its body. */
export interface CardDetail extends CardSummary {
  body: DocNode | null;
}

export interface Comment extends Block {
  type: 'comment';
  body: DocNode | null;
  /**
   * When its text was last changed; absent while it never was. Who changed
   * it is `modifiedBy`.
   */
  editedAt?: number;
}

/** A text a comment had before it was changed: the first is the one it was posted with. */
export interface CommentVersion {
  /** Counted from 1 for the first text; versions no longer kept leave gaps. */
  version: number;
  body: DocNode | null;
  writtenBy: string;
  writtenAt: number;
}

/** A file attached to a card. It is a child block of the card; `fileId` names the bytes. */
export interface Attachment extends Block {
  type: 'attachment';
  fileId: string;
  name: string;
  size: number;
  contentType: string;
}

/** What a card is made from. The server gives it its identifier and times. */
export interface CardDraft {
  title?: string;
  icon?: string;
  properties?: Card['fields']['properties'];
  isTemplate?: boolean;
  body?: DocNode | null;
}

export interface CardPatch {
  title?: string;
  icon?: string;
  /** Properties to set. */
  updatedProperties?: Card['fields']['properties'];
  /** Properties to clear. */
  deletedProperties?: string[];
}

export interface ViewDraft {
  title: string;
  fields: BoardView['fields'];
}

export interface BoardDraft {
  title: string;
  description?: string;
  icon?: string;
  type?: Board['type'];
  cardProperties?: Board['cardProperties'];
}

/** A write of a body: the document and the version it was based on. */
export interface BodyWrite {
  body: DocNode | null;
  /** The `updateAt` of the card or comment as the writer read it. */
  readAt: number;
}

export interface UploadProgress {
  sentBytes: number;
  totalBytes: number;
}

export interface UploadOptions {
  onProgress?: (progress: UploadProgress) => void;
  /** Aborting gives up the upload; what was sent is discarded by the server. */
  signal?: AbortSignal;
}

export interface DownloadOptions {
  /** `sentBytes` counts what has arrived; `totalBytes` is 0 when the length is not known. */
  onProgress?: (progress: UploadProgress) => void;
  signal?: AbortSignal;
}

/** Bytes to be saved, under the name they come with. */
export interface DownloadedFile {
  bytes: Blob;
  name: string;
}

export interface UploadedFile {
  fileId: string;
  name: string;
  size: number;
  contentType: string;
}

// ---------------------------------------------------------------- account

/** What an account is made with. */
export interface Registration {
  /** The name to sign in with. */
  username: string;
  email: string;
  password: string;
  /** The team's invitation; the first account of a server needs none. */
  token?: string;
}

/**
 * Signing in and out, where a backend has accounts: the server has, the one
 * in memory has not. Until somebody is signed in, every read of a backend
 * with accounts is refused with 401.
 */
export interface Account {
  /** `login` is the name to sign in with, or the email address. */
  signIn(login: string, password: string): Promise<void>;
  /** Makes an account. It does not sign in. */
  register(registration: Registration): Promise<void>;
  signOut(): Promise<void>;
  /**
   * Changes the reader's password; `oldPassword` proves they know it. Their
   * other sessions end, this one goes on. A wrong old password is 400 with
   * the reason `invalid_credentials`, too many tries 429.
   */
  changePassword(oldPassword: string, newPassword: string): Promise<void>;
}

// ---------------------------------------------------------------- changes

/**
 * A change that happened on the server, whoever made it: the reader in this
 * tab, the reader in another one, or somebody else. Stores apply these the
 * same way they apply the answer to their own write, so applying one twice
 * does nothing.
 */
export type Change =
  | { kind: 'board'; board: Board }
  | { kind: 'boardDeleted'; boardId: string }
  /** Blocks that were added or changed. A card comes as its summary. */
  | { kind: 'blocks'; boardId: string; blocks: (BoardView | CardSummary | Comment | Attachment)[] }
  | { kind: 'blocksDeleted'; boardId: string; blockIds: string[] }
  | { kind: 'members'; boardId: string; members: BoardMember[] }
  | { kind: 'categories'; categories: Category[] }
  | { kind: 'settings'; settings: AccountSettings }
  /**
   * Changes may have been missed (the connection to the server dropped and
   * is back): read again what is shown. The memory backend never sends it.
   */
  | { kind: 'resync' };

export type ChangeListener = (change: Change) => void;

type ChangedBlock = Extract<Change, { kind: 'blocks' }>['blocks'][number];

/** Whether a changed block is a card. The types of the blocks overlap, so `type` has to be asked. */
export function isCardBlock(block: ChangedBlock): block is CardSummary {
  return block.type === 'card';
}

export function isViewBlock(block: ChangedBlock): block is BoardView {
  return block.type === 'view';
}

// ---------------------------------------------------------------- backend

export interface Backend {
  // The reader and the people around them.
  me(): Promise<User>;
  teams(): Promise<Team[]>;
  /** The people of a team: who can be mentioned or set as a person. */
  users(teamId: string): Promise<User[]>;
  /**
   * The people of a team with these identifiers: for the names `users`
   * did not bring (the server's list of a team is short). Every identifier
   * must be a user's.
   */
  usersById(teamId: string, userIds: string[]): Promise<User[]>;

  // Account settings (server design 5.8) and the pocket of screen state.
  settings(): Promise<AccountSettings>;
  patchSettings(patch: AccountSettingsPatch): Promise<AccountSettings>;
  /** Small things a screen wants to find again on another device. Keys and values are text. */
  screenState(): Promise<Record<string, string>>;
  patchScreenState(updated: Record<string, string>, deleted?: string[]): Promise<void>;

  // Boards.
  boards(teamId: string): Promise<Board[]>;
  board(boardId: string): Promise<Board>;
  /**
   * Makes a board and its first view together, all or none: a board without
   * a view has nothing to show. The reader becomes its admin, and a board
   * that is no template is filed under their default category.
   */
  createBoard(
    teamId: string,
    draft: BoardDraft,
    firstView: ViewDraft,
  ): Promise<{ board: Board; view: BoardView }>;
  patchBoard(boardId: string, patch: BoardPatch): Promise<Board>;
  /**
   * The templates the reader may make a board from: the team's they may
   * see, then the built-in ones every team is offered.
   */
  templates(teamId: string): Promise<Board[]>;
  /**
   * Copies a board with its views and cards (not its comments) into the
   * team: a board from a template, or a template from a board. The reader is
   * the copy's admin, and it is private. The team is said because a built-in
   * template belongs to none of the reader's.
   */
  duplicateBoard(boardId: string, asTemplate: boolean, teamId: string): Promise<Board>;
  /** Copies the welcome board for the reader; answers its identifier. Each call makes another copy. */
  onboard(teamId: string): Promise<string>;
  /**
   * The boards the reader may find whose title has every word of `query` in
   * it, whatever the case: those of `boards`, hidden ones too. No templates.
   */
  searchBoards(teamId: string, query: string): Promise<Board[]>;
  /** Moves the board to the trash. */
  deleteBoard(boardId: string): Promise<void>;
  /** Takes the board, its blocks and its members back out of the trash. */
  restoreBoard(boardId: string): Promise<Board>;
  members(boardId: string): Promise<BoardMember[]>;
  /**
   * Makes a person of the team a member of the board, with the role. For the
   * board's admins. Someone who is a member already keeps their role.
   */
  addMember(boardId: string, userId: string, role: BoardRole): Promise<BoardMember>;
  /** Gives a member another role. The board's last admin stays one (409, `last_admin`). */
  setMemberRole(boardId: string, userId: string, role: BoardRole): Promise<BoardMember>;
  /** Takes a member off the board. Not its last admin (409, `last_admin`). */
  removeMember(boardId: string, userId: string): Promise<void>;
  /** The board's public link. For the board's admins. */
  sharing(boardId: string): Promise<Sharing>;
  /** Turns the public link on or off, or makes it anew. */
  setSharing(boardId: string, change: SharingChange): Promise<Sharing>;
  /** People of the team whose name has `query` in it, whatever the case; ten at most. */
  searchUsers(teamId: string, query: string): Promise<User[]>;
  /**
   * Makes the reader a member of an open board of their team, with the
   * board's minimum role. Until then such a board is readable but its views,
   * cards and members are refused (403).
   */
  joinBoard(boardId: string): Promise<BoardMember>;

  // The reader's sidebar.
  categories(teamId: string): Promise<Category[]>;
  /** A new category goes first. */
  createCategory(teamId: string, draft: CategoryDraft): Promise<Category>;
  /** The default category keeps its name whatever the patch says. */
  patchCategory(teamId: string, categoryId: string, patch: CategoryPatch): Promise<Category>;
  /** Its boards go to the default category, which cannot be deleted. */
  deleteCategory(teamId: string, categoryId: string): Promise<void>;
  /** The reader's order of all their categories. */
  reorderCategories(teamId: string, categoryIds: string[]): Promise<void>;
  /**
   * Files a board under a category, at `index` in the category's own list
   * (`Category.boardIds`, without the board).
   */
  moveBoardToCategory(
    teamId: string,
    boardId: string,
    categoryId: string,
    index: number,
  ): Promise<void>;
  /** Hides a board of a category from the reader's sidebar, or shows it again. */
  setBoardHidden(
    teamId: string,
    categoryId: string,
    boardId: string,
    hidden: boolean,
  ): Promise<void>;

  // Views.
  views(boardId: string): Promise<BoardView[]>;
  createView(boardId: string, draft: ViewDraft): Promise<BoardView>;
  patchView(boardId: string, viewId: string, patch: BlockPatch): Promise<BoardView>;

  // Cards.
  cards(boardId: string): Promise<CardSummary[]>;
  card(boardId: string, cardId: string): Promise<CardDetail>;
  createCard(boardId: string, draft: CardDraft): Promise<CardSummary>;
  patchCard(boardId: string, cardId: string, patch: CardPatch): Promise<CardSummary>;
  /** Replaces the body. Rejects with 409 when the card changed since `readAt`. */
  writeCardBody(boardId: string, cardId: string, write: BodyWrite): Promise<CardDetail>;

  // Comments and attachments of a card.
  comments(boardId: string, cardId: string): Promise<Comment[]>;
  createComment(boardId: string, cardId: string, body: DocNode): Promise<Comment>;
  /**
   * Replaces the text of a comment. Rejects with 409 when the comment changed
   * since `readAt`, and with 400 when the body is empty: a comment has a text.
   */
  patchComment(boardId: string, commentId: string, write: BodyWrite): Promise<Comment>;
  /**
   * What a comment said before each change, the first text first; empty for
   * one never changed. For the board's members (a reader by its link has none).
   */
  commentVersions(boardId: string, commentId: string): Promise<CommentVersion[]>;
  attachments(boardId: string, cardId: string): Promise<Attachment[]>;
  createAttachment(boardId: string, cardId: string, file: UploadedFile): Promise<Attachment>;

  // Any block of a board: a view, a card, a comment, an attachment.
  /** Moves the block and everything under it to the trash. */
  deleteBlock(boardId: string, blockId: string): Promise<void>;
  /** Takes the block and what was deleted with it back out of the trash. */
  restoreBlock(boardId: string, blockId: string): Promise<void>;

  // Files.
  /**
   * Uploads a file of a board. The server takes it in parts (an upload
   * session); the parts and the retrying of one are the implementation's
   * business.
   */
  upload(boardId: string, file: File, options?: UploadOptions): Promise<UploadedFile>;
  /** The bytes of a file. */
  fileBytes(boardId: string, fileId: string): Promise<Blob>;
  /** Where the bytes of a file can be fetched by an `<img>` or a link. */
  fileUrl(boardId: string, fileId: string): string | undefined;

  // Archives of boards.
  /** An archive of the board with its blocks, files and members. For its members. */
  exportBoard(boardId: string, options?: DownloadOptions): Promise<DownloadedFile>;
  /** An archive of every board of the team the reader may see. */
  exportTeam(teamId: string, options?: DownloadOptions): Promise<DownloadedFile>;
  /**
   * Makes the boards of an archive in the team, the reader their admin. An
   * archive over the server's limit is refused before anything is sent (413,
   * with the limit); one that is no archive of this app is 400. The boards
   * are made one after the other: a failure keeps those before it.
   */
  importArchive(teamId: string, file: File, options?: UploadOptions): Promise<void>;

  // Changes as they happen.
  /** Listens to the changes of one board. Returns the function that stops listening. */
  watchBoard(boardId: string, listener: ChangeListener): () => void;
  /** Listens to the changes of the reader's own things: boards they can see, sidebar, settings. */
  watchSelf(listener: ChangeListener): () => void;
}
