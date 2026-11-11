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
import type { CardPreview } from '../editor/card-preview.ts';
import type { DocNode } from '../editor/schema.ts';
import type { DisplaySettings } from '../logic/display.ts';
import type { LanguageSettings } from '../logic/language.ts';
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

import type { TrashReads } from './trash-reads.ts';

export type { Account, Registration } from './account-types.ts';
export type { DeletedItem, Paging, TrashReads } from './trash-reads.ts';

// ----------------------------------------------------------------- errors

export {
  BackendError,
  ERROR_STATUSES,
  isBackendError,
  isConflict,
  problemOf,
  refusedMember,
  type ErrorStatus,
  type InvalidInput,
  type Problem,
} from './problems.ts';

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
 * The reader's account settings: the three of the server design (5.8), the
 * four display settings the plan adds to them, and the language of the
 * interface. The answer always holds every member.
 */
export type AccountSettings = RegionalSettings & DisplaySettings & LanguageSettings;

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
  /**
   * What a gallery shows of the body: only on a card that came in a change
   * message, which carries the body it is made from (`cardPreviews` reads it
   * for a list).
   */
  preview?: CardPreview;
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

export interface Backend extends TrashReads {
  // The reader and the people around them.
  me(): Promise<User>;
  /** Changes the reader's display name, as the server reads it (`patchMe`); `null` is none. */
  patchMe(patch: { displayName: string | null }): Promise<User>;
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
   * Puts the board's card properties in an order: every property once, or
   * 400 and nothing changes. Who may change the properties may (editors).
   * The board's listeners are told the board.
   */
  reorderCardProperties(boardId: string, propertyIds: string[]): Promise<string[]>;
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
  /** Takes the board back out of the trash, filed where each person had it (`TrashReads`). */
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
  /**
   * What a gallery shows of each card's body, by card: the first image and
   * the beginning of the text (`getCards` with `preview`). Asked only for a
   * gallery that is shown.
   */
  cardPreviews(boardId: string): Promise<Map<string, CardPreview>>;
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
