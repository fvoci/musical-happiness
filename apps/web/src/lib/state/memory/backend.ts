/**
 * The backend that keeps everything in memory.
 *
 * It behaves the way the server design says the server will, as far as a
 * screen can tell: it makes the identifiers, checks a body against the
 * allow-list, refuses a stale body with 409, moves deleted things to a trash
 * they can come back from, and tells every listener about every change.
 * Lists come back in no useful order on purpose (see `scramble`): a screen
 * that leans on the order of an answer is caught here, not in production.
 *
 * Besides the interface it has a few handles for tests and for the
 * development page: `as` (the same world seen by another user), `failNext`
 * and `latency`.
 */
import { cardPreviewOf, type CardPreview } from '../../editor/card-preview.ts';
import { storedSize, validateDoc, type DocNode } from '../../editor/schema.ts';
import { readDisplaySettings, type DisplaySettings } from '../../logic/display.ts';
import { isTimeZone, readRegionalSettings } from '../../logic/regional.ts';
import { DEFAULT_ACCOUNT_SETTINGS } from '../../logic/account-settings.ts';
import { readLanguageSettings } from '../../logic/language.ts';
import {
  createBoard,
  type BlockPatch,
  type Board,
  type BoardMember,
  type BoardPatch,
  type BoardView,
} from '../../logic/wire.ts';
import {
  BackendError,
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
  type DeletedItem,
  type DownloadedFile,
  type DownloadOptions,
  type Paging,
  type Sharing,
  type SharingChange,
  type WorkspaceInfo,
  type UploadedFile,
  type UploadOptions,
  type User,
  type ViewDraft,
} from '../backend.ts';

import { applyBoardPatch, applyCardPatch, applyViewPatch } from '../patches.ts';

import {
  noteChange,
  versionsOf,
  lastCommentAt,
  mayChangeComment,
  mayRemoveComments,
  roleOn,
  type Role,
} from './comments.ts';
import { changeDisplayName } from './account.ts';
import { checkRelations, renameCopied } from './references.ts';
import { fail } from './refuse.ts';
import {
  deletedBoardsOf,
  deletedCardsOf,
  deletion,
  mayManageCards,
  placesOf,
  takeBoardBack,
} from './trash.ts';
import type { StoredBlock, StoredCard, StoredView, World } from './world.ts';

import {
  attachmentOf,
  commentOf,
  copy,
  detailOf,
  imageFileIds,
  inPropertyOrder,
  LIMITS,
  MEMORY_ARCHIVE,
  memberOf,
  scramble,
  SCREEN_STATE_KEY,
  summaryOf,
  viewOf,
  wireOf,
} from './shapes.ts';

export { LIMITS, memberOf };

type MethodName = keyof Backend;

export class MemoryBackend implements Backend {
  readonly world: World;
  readonly userId: string;
  /** Milliseconds every call waits before it answers. */
  latency = 0;
  readonly #failures = new Map<MethodName, BackendError>();

  constructor(world: World, userId: string) {
    this.world = world;
    this.userId = userId;
  }

  /** The same world, seen and changed by another user. */
  as(userId: string): MemoryBackend {
    if (!this.world.users.has(userId)) throw new Error(`no user ${userId}`);
    return new MemoryBackend(this.world, userId);
  }

  /** Makes the next call of `method` reject with `error`, once. */
  failNext(method: MethodName, error: BackendError): void {
    this.#failures.set(method, error);
  }

  /** `#enter`, then 404 unless `workspaceId` names the reader's workspace. */
  async #enterWorkspace(method: MethodName, workspaceId: string): Promise<void> {
    await this.#enter(method);
    if (workspaceId !== this.world.workspace.id) fail(404, 'no such workspace');
  }

  /**
   * The wait every call starts with, and the failure a test asked for. A call
   * that was given up while it waited (`signal`) ends here as given up: it
   * does not take the failure meant for the next call.
   */
  async #enter(method: MethodName, signal?: AbortSignal): Promise<void> {
    if (this.latency > 0) {
      await new Promise((resolve) => {
        setTimeout(resolve, this.latency);
      });
    } else {
      await Promise.resolve();
    }
    if (signal?.aborted) throw new BackendError(0, { detail: 'the call was given up' });
    const failure = this.#failures.get(method);
    if (failure) {
      this.#failures.delete(method);
      throw failure;
    }
  }

  #board(boardId: string): Board {
    return this.world.boards.get(boardId) ?? fail(404, 'no such board');
  }

  #block(boardId: string, blockId: string, type?: StoredBlock['type']): StoredBlock {
    const block = this.world.blocks.get(blockId);
    if (block?.boardId !== boardId || (type && block.type !== type)) {
      fail(404, `no such ${type ?? 'block'}`);
    }
    return block;
  }

  #roleOn(boardId: string): Role {
    return roleOn(this.world.members.get(boardId) ?? [], this.userId);
  }

  /** The reader's role on the board; one they are no member of is not there for them, unless open (403). */
  #memberRole(boardId: string): Exclude<Role, ''> {
    const board = this.#board(boardId);
    const role = this.#roleOn(boardId);
    if (role !== '') return role;
    if (board.type === 'O') fail(403, 'access denied to board');
    fail(404, 'no such board');
  }

  /**
   * A card as the card routes answer it: with when its last comment was
   * posted. A change message has no such time (`wireOf`), as on the server.
   */
  #commented<T extends CardSummary>(card: T): T {
    return { ...card, lastCommentAt: lastCommentAt(this.world.childrenOf(card.id)) };
  }

  #memberIds(boardId: string): string[] {
    return (this.world.members.get(boardId) ?? []).map((member) => member.userId);
  }

  #checkBody(boardId: string, body: DocNode | null): DocNode | null {
    if (body === null) return null;
    const problems = validateDoc(body);
    if (problems.length > 0) fail(400, problems[0] ?? 'invalid body', { field: 'body' });
    if (storedSize(body) > LIMITS.bodyBytes) {
      fail(400, 'the body is too large', { field: 'body', limit: LIMITS.bodyBytes });
    }
    for (const fileId of imageFileIds(body)) {
      if (this.world.files.get(fileId)?.boardId !== boardId) {
        fail(400, 'an image names a file that is not of this board', { field: 'body' });
      }
    }
    // An empty paragraph alone is an empty body, stored as none.
    const only = body.content?.length === 1 ? body.content[0] : undefined;
    const empty = only?.type === 'paragraph' && (only.content?.length ?? 0) === 0;
    return empty ? null : copy(body);
  }

  #touch(block: StoredBlock): void {
    block.updateAt = this.world.now();
    block.modifiedBy = this.userId;
  }

  #tellBlocks(boardId: string, blocks: readonly StoredBlock[]): void {
    this.world.tellBoard(boardId, { kind: 'blocks', boardId, blocks: blocks.map(wireOf) });
  }

  #tellBoard(board: Board): void {
    this.world.tellBoard(board.id, { kind: 'board', board: copy(board) });
    this.world.tellUsers(this.#memberIds(board.id), { kind: 'board', board: copy(board) });
  }

  #newBlock(boardId: string, parentId: string, type: StoredBlock['type']): StoredBlock {
    const now = this.world.now();
    return {
      id: this.world.newId(),
      boardId,
      parentId,
      createdBy: this.userId,
      modifiedBy: this.userId,
      schema: 1,
      type,
      title: '',
      fields: {},
      createAt: now,
      updateAt: now,
      deleteAt: 0,
    };
  }

  // ------------------------------------------------------------- people

  async me(): Promise<User> {
    await this.#enter('me');
    return copy(this.world.users.get(this.userId) ?? fail(401, 'no session'));
  }

  async patchMe(patch: { displayName: string | null }): Promise<User> {
    await this.#enter('patchMe');
    return copy(changeDisplayName(this.world, this.userId, patch.displayName));
  }

  async workspaces(): Promise<WorkspaceInfo[]> {
    await this.#enter('workspaces');
    return [copy(this.world.workspace)];
  }

  async users(workspaceId: string): Promise<User[]> {
    await this.#enterWorkspace('users', workspaceId);
    return scramble([...this.world.users.values()]).map((user) => {
      const { isAdmin: _isAdmin, ...rest } = copy(user);
      return rest;
    });
  }

  /** As strict as the server: no empty list, no identifier twice, every one a user's. */
  async usersById(workspaceId: string, userIds: string[]): Promise<User[]> {
    const users = await this.users(workspaceId);
    if (userIds.length === 0) fail(400, 'no users asked for');
    if (new Set(userIds).size !== userIds.length) fail(404, 'a user asked for twice');
    if (userIds.some((id) => !this.world.users.has(id))) fail(404, 'no such user');
    return users.filter((user) => userIds.includes(user.id));
  }

  // ----------------------------------------------------------- settings

  #settings(): AccountSettings {
    return this.world.settings.get(this.userId) ?? { ...DEFAULT_ACCOUNT_SETTINGS };
  }

  async settings(): Promise<AccountSettings> {
    await this.#enter('settings');
    return copy(this.#settings());
  }

  async patchSettings(patch: AccountSettingsPatch): Promise<AccountSettings> {
    await this.#enter('patchSettings');
    const defaults = DEFAULT_ACCOUNT_SETTINGS;
    const merged: Record<string, unknown> = { ...this.#settings() };
    for (const [name, value] of Object.entries(patch)) {
      if (!(name in defaults)) fail(400, `unknown setting ${name}`, { field: name });
      merged[name] = value ?? defaults[name as keyof AccountSettings];
    }
    const zone = merged.timeZone;
    if (typeof zone !== 'string' || (zone !== 'auto' && !isTimeZone(zone))) {
      fail(400, 'unknown time zone', { field: 'timeZone' });
    }
    const display: DisplaySettings = readDisplaySettings(merged);
    const regional = readRegionalSettings(merged);
    const next: AccountSettings = { ...regional, ...display, ...readLanguageSettings(merged) };
    // A value the reader functions fell back on was not one of the allowed ones.
    for (const name of Object.keys(patch) as (keyof AccountSettings)[]) {
      if (patch[name] !== null && patch[name] !== undefined && next[name] !== patch[name]) {
        fail(400, `invalid value for ${name}`, { field: name });
      }
    }
    this.world.settings.set(this.userId, next);
    this.world.tellUsers([this.userId], { kind: 'settings', settings: copy(next) });
    return copy(next);
  }

  async screenState(): Promise<Record<string, string>> {
    await this.#enter('screenState');
    return { ...this.world.screenState.get(this.userId) };
  }

  async patchScreenState(updated: Record<string, string>, deleted: string[] = []): Promise<void> {
    await this.#enter('patchScreenState');
    const next = Object.fromEntries(
      Object.entries(this.world.screenState.get(this.userId) ?? {}).filter(
        ([key]) => !deleted.includes(key),
      ),
    );
    for (const [key, value] of Object.entries(updated)) {
      if (!SCREEN_STATE_KEY.test(key)) {
        fail(400, 'invalid key', {
          field: key,
          within: 'updatedFields',
          reason: 'invalid_characters',
        });
      }
      if (new TextEncoder().encode(value).length > LIMITS.screenStateValueBytes) {
        fail(400, 'value too long', {
          field: key,
          within: 'updatedFields',
          reason: 'too_long',
          limit: LIMITS.screenStateValueBytes,
        });
      }
      next[key] = value;
    }
    if (Object.keys(next).length > LIMITS.screenStateKeys) {
      fail(400, 'too many keys', { limit: LIMITS.screenStateKeys });
    }
    this.world.screenState.set(this.userId, next);
  }

  // ------------------------------------------------------------- boards

  async boards(workspaceId: string): Promise<Board[]> {
    await this.#enterWorkspace('boards', workspaceId);
    const mine = [...this.world.boards.values()].filter((board) =>
      this.#memberIds(board.id).includes(this.userId),
    );
    return scramble(mine).map(copy);
  }

  async board(boardId: string): Promise<Board> {
    await this.#enter('board');
    return copy(this.#board(boardId));
  }

  async createBoard(
    workspaceId: string,
    draft: BoardDraft,
    firstView: ViewDraft,
  ): Promise<{ board: Board; view: BoardView }> {
    await this.#enterWorkspace('createBoard', workspaceId);
    const now = this.world.now();
    const board: Board = {
      ...createBoard(),
      id: this.world.newId(),
      workspaceId,
      createdBy: this.userId,
      modifiedBy: this.userId,
      type: draft.type ?? 'P',
      minimumRole: 'editor',
      title: draft.title,
      description: draft.description ?? '',
      icon: draft.icon ?? '',
      createAt: now,
      updateAt: now,
    };
    if (draft.cardProperties) board.cardProperties = copy(draft.cardProperties);
    this.world.boards.set(board.id, board);
    this.world.members.set(board.id, [memberOf(board.id, this.userId, 'admin')]);
    this.#fileAsNew(this.userId, board);
    const view = this.#newBlock(board.id, board.id, 'view');
    view.title = firstView.title;
    view.fields = copy(firstView.fields);
    this.world.blocks.set(view.id, view);
    this.#tellBoard(board);
    this.#tellBlocks(board.id, [view]);
    return { board: copy(board), view: viewOf(view) };
  }

  async patchBoard(boardId: string, patch: BoardPatch): Promise<Board> {
    await this.#enter('patchBoard');
    const board = applyBoardPatch(this.#board(boardId), patch);
    this.world.boards.set(boardId, board);
    board.updateAt = this.world.now();
    board.modifiedBy = this.userId;
    this.#tellBoard(board);
    return copy(board);
  }

  async reorderCardProperties(boardId: string, propertyIds: string[]): Promise<string[]> {
    await this.#enter('reorderCardProperties');
    const role = this.#memberRole(boardId);
    if (role !== 'admin' && role !== 'editor') {
      fail(403, 'access denied to modifying board properties');
    }
    const board = inPropertyOrder(this.#board(boardId), propertyIds);
    board.updateAt = this.world.now();
    board.modifiedBy = this.userId;
    this.world.boards.set(boardId, board);
    this.#tellBoard(board);
    return [...propertyIds];
  }

  async deleteBoard(boardId: string): Promise<void> {
    await this.#enter('deleteBoard');
    const board = this.#board(boardId);
    const members = this.world.members.get(boardId) ?? [];
    const blocks = this.world.blocksOf(boardId);
    const places = placesOf(this.world, boardId);
    this.world.deletedBoards.set(boardId, { board, members, blocks, places, ...deletion(this) });
    for (const block of blocks) this.world.blocks.delete(block.id);
    this.world.boards.delete(boardId);
    this.world.members.delete(boardId);
    for (const categories of this.world.categories.values()) {
      for (const category of categories) {
        category.boardIds = category.boardIds.filter((id) => id !== boardId);
        category.hiddenBoardIds = category.hiddenBoardIds.filter((id) => id !== boardId);
      }
    }
    const userIds = members.map((member) => member.userId);
    this.world.tellBoard(boardId, { kind: 'boardDeleted', boardId });
    this.world.tellUsers(userIds, { kind: 'boardDeleted', boardId });
  }

  /** As the server: for who was its admin; filed again where each person had it. */
  async restoreBoard(boardId: string): Promise<Board> {
    await this.#enter('restoreBoard');
    const board = takeBoardBack(this.world, this.userId, boardId);
    this.#tellBoard(board);
    return copy(board);
  }

  async deletedBoards(workspaceId: string, paging?: Paging): Promise<DeletedItem[]> {
    await this.#enterWorkspace('deletedBoards', workspaceId);
    return deletedBoardsOf(this.world, this.userId, workspaceId, paging);
  }

  /**
   * As the server matches: every word of the term in the title, whatever the
   * case; oldest first. Among the boards this backend lists (`boards`: the
   * reader's), not the workspace's open ones the server adds.
   */
  async searchBoards(workspaceId: string, query: string): Promise<Board[]> {
    await this.#enterWorkspace('searchBoards', workspaceId);
    const words = query
      .toLowerCase()
      .split(' ')
      .filter((word) => word !== '');
    if (words.length === 0) return [];
    return [...this.world.boards.values()]
      .filter(
        (board) =>
          !board.isTemplate &&
          this.#memberIds(board.id).includes(this.userId) &&
          words.every((word) => board.title.toLowerCase().includes(word)),
      )
      .sort((first, second) => first.createAt - second.createAt)
      .map(copy);
  }

  /** As the server: the workspace's templates, then the built-in ones, each the oldest first. */
  async templates(workspaceId: string): Promise<Board[]> {
    await this.#enterWorkspace('templates', workspaceId);
    const all = [...this.world.boards.values()]
      .filter((board) => board.isTemplate)
      .sort((first, second) => first.createAt - second.createAt);
    return [
      ...all.filter((board) => board.isBuiltin !== true),
      ...all.filter((board) => board.isBuiltin === true),
    ].map(copy);
  }

  async duplicateBoard(boardId: string, asTemplate: boolean, workspaceId: string): Promise<Board> {
    await this.#enterWorkspace('duplicateBoard', workspaceId);
    return this.#copyBoard(this.#board(boardId), asTemplate);
  }

  /** As the server: a copy of the built-in welcome template, without its mark. */
  async onboard(workspaceId: string): Promise<string> {
    await this.#enterWorkspace('onboard', workspaceId);
    const welcome =
      [...this.world.boards.values()].find(
        (board) =>
          board.isTemplate && board.isBuiltin === true && board.properties.onboarding !== undefined,
      ) ?? fail(404, 'no welcome board');
    const { onboarding: _onboarding, ...properties } = welcome.properties;
    return this.#insertBoard({ ...welcome, isTemplate: false, properties }, this.#copied(welcome))
      .id;
  }

  /** The blocks a copy of a board takes: all but its comments. */
  #copied(board: Board): StoredBlock[] {
    return this.world.blocksOf(board.id).filter((block) => block.type !== 'comment');
  }

  /**
   * A copy of a board with its views, cards and attachments (comments stay
   * behind), every identifier new.
   */
  #copyBoard(source: Board, asTemplate: boolean): Board {
    // The server's titles: a copy of the same kind is "… copy", a template
    // made from a board "New board template", a board from a template keeps it.
    const title =
      source.isTemplate === asTemplate
        ? `${source.title} copy`
        : asTemplate
          ? 'New board template'
          : source.title;
    return this.#insertBoard({ ...source, isTemplate: asTemplate, title }, this.#copied(source));
  }

  /**
   * Stores a board and its blocks under new identifiers, the reader its
   * admin. As on the server, only the references it renames follow: a
   * block's board and parent, and what `renameCopied` renames.
   */
  #insertBoard(source: Board, blocks: readonly StoredBlock[]): Board {
    const now = this.world.now();
    const ids = new Map<string, string>([[source.id, this.world.newId()]]);
    for (const block of blocks) ids.set(block.id, this.world.newId());
    const renamed = (id: string): string => ids.get(id) ?? id;
    const cardIds = new Set(blocks.filter((block) => block.type === 'card').map(({ id }) => id));
    const board: Board = {
      ...copy(source),
      id: renamed(source.id),
      workspaceId: this.world.workspace.id,
      createdBy: this.userId,
      modifiedBy: this.userId,
      type: 'P',
      isBuiltin: false,
      createAt: now,
      updateAt: now,
    };
    this.world.boards.set(board.id, board);
    this.world.members.set(board.id, [memberOf(board.id, this.userId, 'admin')]);
    this.#fileAsNew(this.userId, board);
    for (const block of blocks) {
      const made = copy(block);
      renameCopied(source, made, ids, cardIds);
      this.world.blocks.set(renamed(block.id), {
        ...made,
        id: renamed(block.id),
        boardId: board.id,
        parentId: made.parentId === source.id ? board.id : renamed(made.parentId),
        createAt: now,
        updateAt: now,
      });
    }
    this.#tellBoard(board);
    return copy(board);
  }

  // ----------------------------------------------------------- archives

  /**
   * An archive of the memory backend: lines of JSON, as the server's
   * `board.jsonl` is, but not its ZIP. Only this backend reads it back.
   */
  async exportBoard(boardId: string, options?: DownloadOptions): Promise<DownloadedFile> {
    await this.#enter('exportBoard', options?.signal);
    const board = this.#board(boardId);
    // For its members, as on the server (an open board is readable, its parts are not).
    if (!this.#memberIds(boardId).includes(this.userId)) fail(403, 'not a member of the board');
    return this.#archiveOf([board], options);
  }

  async exportWorkspace(workspaceId: string, options?: DownloadOptions): Promise<DownloadedFile> {
    await this.#enter('exportWorkspace', options?.signal);
    if (workspaceId !== this.world.workspace.id) fail(404, 'no such workspace');
    // As the server: the reader's boards and the workspace's open ones.
    const boards = [...this.world.boards.values()].filter(
      (board) =>
        !board.isTemplate &&
        (board.type === 'O' || this.#memberIds(board.id).includes(this.userId)),
    );
    return this.#archiveOf(boards, options);
  }

  #archiveOf(boards: readonly Board[], options: DownloadOptions | undefined): DownloadedFile {
    const lines = [JSON.stringify({ version: MEMORY_ARCHIVE })];
    for (const board of boards) {
      lines.push(JSON.stringify({ type: 'board', data: board }));
      for (const block of this.world.blocksOf(board.id)) {
        lines.push(JSON.stringify({ type: 'block', data: block }));
      }
    }
    const bytes = new Blob([`${lines.join('\n')}\n`], { type: 'application/octet-stream' });
    options?.onProgress?.({ sentBytes: bytes.size, totalBytes: bytes.size });
    const day = new Date(this.world.now()).toISOString().slice(0, 10);
    return { bytes, name: `archive-${day}.boardarchive` };
  }

  async importArchive(workspaceId: string, file: File, options?: UploadOptions): Promise<void> {
    await this.#enter('importArchive', options?.signal);
    if (workspaceId !== this.world.workspace.id) fail(404, 'no such workspace');
    let lines: { type?: string; version?: string; data?: unknown }[];
    try {
      lines = (await file.text())
        .split('\n')
        .filter((line) => line.trim() !== '')
        .map((line) => JSON.parse(line) as { type?: string; version?: string; data?: unknown });
    } catch {
      fail(400, 'not an archive');
    }
    if (lines[0]?.version !== MEMORY_ARCHIVE) {
      fail(400, 'not an archive of this app');
    }
    options?.onProgress?.({ sentBytes: file.size, totalBytes: file.size });
    const boards = lines.filter((line) => line.type === 'board').map((line) => line.data as Board);
    const blocks = lines
      .filter((line) => line.type === 'block')
      .map((line) => line.data as StoredBlock);
    for (const board of boards) {
      this.#insertBoard(
        board,
        blocks.filter((block) => block.boardId === board.id),
      );
    }
  }

  async joinBoard(boardId: string): Promise<BoardMember> {
    await this.#enter('joinBoard');
    const board = this.#board(boardId);
    if (board.isTemplate) fail(403, 'a template cannot be joined');
    if (board.type !== 'O') fail(404, 'no such open board');
    const members = this.world.members.get(boardId) ?? [];
    const existing = members.find((member) => member.userId === this.userId);
    if (existing) return copy(existing);
    const role = board.minimumRole === 'admin' ? 'editor' : board.minimumRole || 'editor';
    const member = memberOf(boardId, this.userId, role);
    this.world.members.set(boardId, [...members, member]);
    this.world.tellBoard(boardId, {
      kind: 'members',
      boardId,
      members: copy([...members, member]),
    });
    return copy(member);
  }

  async members(boardId: string): Promise<BoardMember[]> {
    await this.#enter('members');
    this.#board(boardId);
    return scramble(this.world.members.get(boardId) ?? []).map(copy);
  }

  async addMember(boardId: string, userId: string, role: BoardRole): Promise<BoardMember> {
    await this.#enter('addMember');
    this.#board(boardId);
    if (!this.world.users.has(userId)) fail(400, 'no such user', { field: 'userId' });
    const members = this.world.members.get(boardId) ?? [];
    const existing = members.find((member) => member.userId === userId);
    // As the server: somebody who is a member already keeps their role.
    if (existing) return copy(existing);
    const member = memberOf(boardId, userId, role);
    this.#setMembers(boardId, [...members, member]);
    this.#fileAsNew(userId, this.#board(boardId));
    this.world.tellUsers([userId], { kind: 'board', board: copy(this.#board(boardId)) });
    return copy(member);
  }

  async setMemberRole(boardId: string, userId: string, role: BoardRole): Promise<BoardMember> {
    await this.#enter('setMemberRole');
    this.#board(boardId);
    const members = this.world.members.get(boardId) ?? [];
    if (!members.some((member) => member.userId === userId)) fail(404, 'no such member');
    const member = memberOf(boardId, userId, role);
    const next = members.map((one) => (one.userId === userId ? member : one));
    this.#keepAnAdmin(next);
    this.#setMembers(boardId, next);
    return copy(member);
  }

  async removeMember(boardId: string, userId: string): Promise<void> {
    await this.#enter('removeMember');
    this.#board(boardId);
    const members = this.world.members.get(boardId) ?? [];
    const next = members.filter((member) => member.userId !== userId);
    if (next.length === members.length) return;
    this.#keepAnAdmin(next);
    this.#setMembers(boardId, next);
    this.world.tellUsers([userId], { kind: 'boardDeleted', boardId });
  }

  /** A board keeps an admin, as on the server (409, `last_admin`). */
  #keepAnAdmin(members: readonly BoardMember[]): void {
    if (!members.some((member) => member.schemeAdmin)) {
      fail(409, 'a board keeps its last admin', { reason: 'last_admin' });
    }
  }

  #setMembers(boardId: string, members: BoardMember[]): void {
    this.world.members.set(boardId, members);
    this.world.tellBoard(boardId, { kind: 'members', boardId, members: copy(members) });
  }

  async sharing(boardId: string): Promise<Sharing> {
    await this.#enter('sharing');
    this.#board(boardId);
    return copy(this.world.sharing.get(boardId) ?? { enabled: false, token: '' });
  }

  async setSharing(boardId: string, change: SharingChange): Promise<Sharing> {
    await this.#enter('setSharing');
    this.#board(boardId);
    const before = this.world.sharing.get(boardId);
    const token =
      !before?.token || change.regenerate ? this.world.newId().replaceAll('-', '') : before.token;
    const sharing = { enabled: change.enabled, token };
    this.world.sharing.set(boardId, sharing);
    return copy(sharing);
  }

  /** As the server: in either name, whatever the case, the oldest first, ten at most. */
  async searchUsers(workspaceId: string, query: string): Promise<User[]> {
    await this.#enter('searchUsers');
    const wanted = query.trim().toLowerCase();
    return (await this.users(workspaceId))
      .filter((user) =>
        [user.username, user.displayName].some((name) => name.toLowerCase().includes(wanted)),
      )
      .sort((first, second) => first.createAt - second.createAt)
      .slice(0, 10);
  }

  // --------------------------------------------------------- categories

  #categories(): Category[] {
    let mine = this.world.categories.get(this.userId);
    if (!mine) {
      mine = [];
      this.world.categories.set(this.userId, mine);
    }
    return mine;
  }

  #category(categoryId: string): Category {
    return (
      this.#categories().find((category) => category.id === categoryId) ??
      fail(404, 'no such category')
    );
  }

  /**
   * Files a board under a user's default category, as the server does for a
   * new board (not a template) and for a new member of one.
   */
  #fileAsNew(userId: string, board: Board): void {
    if (board.isTemplate) return;
    const theirs = this.world.categories.get(userId) ?? [];
    const fallback = theirs.find((category) => category.type === 'system');
    if (!fallback || theirs.some((category) => category.boardIds.includes(board.id))) return;
    fallback.boardIds.push(board.id);
    this.world.tellUsers([userId], { kind: 'categories', categories: copy(theirs) });
  }

  #tellCategories(): void {
    this.world.tellUsers([this.userId], {
      kind: 'categories',
      categories: copy(this.#categories()),
    });
  }

  async categories(workspaceId: string): Promise<Category[]> {
    await this.#enterWorkspace('categories', workspaceId);
    return scramble(this.#categories()).map(copy);
  }

  async createCategory(workspaceId: string, draft: CategoryDraft): Promise<Category> {
    await this.#enterWorkspace('createCategory', workspaceId);
    if (draft.name.trim() === '') fail(400, 'a category needs a name', { field: 'name' });
    const now = this.world.now();
    const mine = this.#categories();
    // A new category goes first, as the original's did.
    for (const category of mine) category.sortOrder += 10;
    const category: Category = {
      id: this.world.newId(),
      name: draft.name,
      userId: this.userId,
      workspaceId,
      type: 'custom',
      collapsed: false,
      sortOrder: 0,
      boardIds: [],
      hiddenBoardIds: [],
      createAt: now,
      updateAt: now,
    };
    mine.push(category);
    this.#tellCategories();
    return copy(category);
  }

  async patchCategory(
    workspaceId: string,
    categoryId: string,
    patch: CategoryPatch,
  ): Promise<Category> {
    await this.#enterWorkspace('patchCategory', workspaceId);
    const category = this.#category(categoryId);
    if (patch.name !== undefined) {
      if (patch.name.trim() === '') fail(400, 'a category needs a name', { field: 'name' });
      // The default category keeps its name, as the server's does.
      if (category.type !== 'system') category.name = patch.name;
    }
    if (patch.collapsed !== undefined) category.collapsed = patch.collapsed;
    category.updateAt = this.world.now();
    this.#tellCategories();
    return copy(category);
  }

  async deleteCategory(workspaceId: string, categoryId: string): Promise<void> {
    await this.#enterWorkspace('deleteCategory', workspaceId);
    const category = this.#category(categoryId);
    if (category.type === 'system') fail(403, 'the default category cannot be deleted');
    const mine = this.#categories();
    // Its boards fall back to the default category, those it hid still hidden.
    const fallback = mine.find((candidate) => candidate.type === 'system');
    fallback?.boardIds.push(...category.boardIds);
    fallback?.hiddenBoardIds.push(...category.hiddenBoardIds);
    mine.splice(mine.indexOf(category), 1);
    this.#tellCategories();
  }

  async reorderCategories(workspaceId: string, categoryIds: string[]): Promise<void> {
    await this.#enter('reorderCategories');
    const mine = this.#categories();
    const known = new Set(mine.map((category) => category.id));
    if (categoryIds.length !== mine.length || !categoryIds.every((id) => known.has(id))) {
      fail(400, 'the order must name every category once');
    }
    for (const category of mine) category.sortOrder = categoryIds.indexOf(category.id) * 10;
    this.#tellCategories();
  }

  async moveBoardToCategory(
    workspaceId: string,
    boardId: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
    await this.#enterWorkspace('moveBoardToCategory', workspaceId);
    this.#board(boardId);
    const target = this.#category(categoryId);
    for (const category of this.#categories()) {
      category.boardIds = category.boardIds.filter((id) => id !== boardId);
    }
    const at = Math.max(0, Math.min(index, target.boardIds.length));
    target.boardIds.splice(at, 0, boardId);
    this.#tellCategories();
  }

  async setBoardHidden(
    workspaceId: string,
    categoryId: string,
    boardId: string,
    hidden: boolean,
  ): Promise<void> {
    await this.#enterWorkspace('setBoardHidden', workspaceId);
    const category = this.#category(categoryId);
    const others = category.hiddenBoardIds.filter((id) => id !== boardId);
    // As the server: a board the category does not hold is no error.
    category.hiddenBoardIds =
      hidden && category.boardIds.includes(boardId) ? [...others, boardId] : others;
    this.#tellCategories();
  }

  // -------------------------------------------------------------- views

  async views(boardId: string): Promise<BoardView[]> {
    await this.#enter('views');
    this.#board(boardId);
    return scramble(this.world.blocksOf(boardId).filter((block) => block.type === 'view')).map(
      viewOf,
    );
  }

  async createView(boardId: string, draft: ViewDraft): Promise<BoardView> {
    await this.#enter('createView');
    this.#board(boardId);
    const block = this.#newBlock(boardId, boardId, 'view');
    block.title = draft.title;
    block.fields = copy(draft.fields);
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    return viewOf(block);
  }

  async patchView(boardId: string, viewId: string, patch: BlockPatch): Promise<BoardView> {
    await this.#enter('patchView');
    const stored = this.#block(boardId, viewId, 'view') as StoredView;
    const view: StoredView = { ...stored, ...applyViewPatch(stored, patch) };
    this.world.blocks.set(viewId, view);
    this.#touch(view);
    this.#tellBlocks(boardId, [view]);
    return viewOf(view);
  }

  // -------------------------------------------------------------- cards

  async cards(boardId: string): Promise<CardSummary[]> {
    await this.#enter('cards');
    this.#board(boardId);
    const cards = this.world.blocksOf(boardId).filter((block) => block.type === 'card');
    return scramble(cards).map((card) => this.#commented(summaryOf(card as StoredCard)));
  }

  async cardPreviews(boardId: string): Promise<Map<string, CardPreview>> {
    await this.#enter('cardPreviews');
    this.#board(boardId);
    const cards = this.world.blocksOf(boardId).filter((block) => block.type === 'card');
    return new Map(cards.map((card) => [card.id, cardPreviewOf(card.body)]));
  }

  async card(boardId: string, cardId: string): Promise<CardDetail> {
    await this.#enter('card');
    return this.#commented(detailOf(this.#block(boardId, cardId, 'card') as StoredCard));
  }

  async createCard(boardId: string, draft: CardDraft): Promise<CardSummary> {
    await this.#enter('createCard');
    const board = this.#board(boardId);
    const block = this.#newBlock(boardId, boardId, 'card');
    block.title = draft.title ?? '';
    block.fields = {
      icon: draft.icon ?? '',
      isTemplate: draft.isTemplate ?? false,
      properties: copy(draft.properties ?? {}),
    };
    block.body = this.#checkBody(boardId, draft.body ?? null);
    checkRelations(this.world, board, block, undefined, 'properties');
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    return this.#commented(summaryOf(block as StoredCard));
  }

  async patchCard(boardId: string, cardId: string, patch: CardPatch): Promise<CardSummary> {
    await this.#enter('patchCard');
    const before = this.#block(boardId, cardId, 'card') as StoredCard;
    const card = applyCardPatch(before, patch);
    checkRelations(this.world, this.#board(boardId), card, before, 'updatedProperties');
    this.world.blocks.set(cardId, card);
    this.#touch(card);
    this.#tellBlocks(boardId, [card]);
    return this.#commented(summaryOf(card));
  }

  async writeCardBody(boardId: string, cardId: string, write: BodyWrite): Promise<CardDetail> {
    await this.#enter('writeCardBody');
    const card = this.#block(boardId, cardId, 'card') as StoredCard;
    const body = this.#checkBody(boardId, write.body);
    if (card.updateAt !== write.readAt) {
      fail(409, 'the card changed since it was read');
    }
    card.body = body;
    this.#touch(card);
    this.#tellBlocks(boardId, [card]);
    return this.#commented(detailOf(card));
  }

  // ------------------------------------------- comments and attachments

  async comments(boardId: string, cardId: string): Promise<Comment[]> {
    await this.#enter('comments');
    this.#block(boardId, cardId, 'card');
    return scramble(this.world.childrenOf(cardId).filter((block) => block.type === 'comment')).map(
      commentOf,
    );
  }

  async createComment(boardId: string, cardId: string, body: DocNode): Promise<Comment> {
    await this.#enter('createComment');
    this.#block(boardId, cardId, 'card');
    const checked = this.#checkBody(boardId, body);
    if (checked === null) fail(400, 'a comment needs a text', { field: 'body' });
    const block = this.#newBlock(boardId, cardId, 'comment');
    block.body = checked;
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    return commentOf(block);
  }

  async patchComment(boardId: string, commentId: string, write: BodyWrite): Promise<Comment> {
    await this.#enter('patchComment');
    // The role first, as on the server: who cannot comment learns nothing of the block.
    const role = this.#memberRole(boardId);
    if (role === 'viewer') fail(403, 'access denied to make board changes');
    const block = this.#block(boardId, commentId, 'comment');
    if (!mayChangeComment(role, block.createdBy === this.userId)) {
      fail(403, "access denied to change other users' comments");
    }
    const checked = this.#checkBody(boardId, write.body);
    if (checked === null) fail(400, 'a comment needs a text', { field: 'body' });
    if (block.updateAt !== write.readAt) {
      fail(409, 'the comment changed since it was read');
    }
    const before = block.body ?? null;
    block.body = checked;
    this.#touch(block);
    noteChange(this.world, block, before, this.userId);
    this.#tellBlocks(boardId, [block]);
    return commentOf(block);
  }

  async commentVersions(boardId: string, commentId: string): Promise<CommentVersion[]> {
    await this.#enter('commentVersions');
    this.#memberRole(boardId);
    return versionsOf(this.world, boardId, commentId);
  }

  async attachments(boardId: string, cardId: string): Promise<Attachment[]> {
    await this.#enter('attachments');
    this.#block(boardId, cardId, 'card');
    return scramble(
      this.world.childrenOf(cardId).filter((block) => block.type === 'attachment'),
    ).map(attachmentOf);
  }

  async createAttachment(boardId: string, cardId: string, file: UploadedFile): Promise<Attachment> {
    await this.#enter('createAttachment');
    this.#block(boardId, cardId, 'card');
    const stored = this.world.files.get(file.fileId);
    if (stored?.boardId !== boardId) fail(400, 'no such file on this board', { field: 'fileId' });
    const block = this.#newBlock(boardId, cardId, 'attachment');
    block.title = stored.name;
    block.file = {
      fileId: stored.fileId,
      name: stored.name,
      size: stored.size,
      contentType: stored.contentType,
    };
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    return attachmentOf(block);
  }

  // ------------------------------------------------------------- blocks

  async deleteBlock(boardId: string, blockId: string): Promise<void> {
    await this.#enter('deleteBlock');
    const block = this.#block(boardId, blockId);
    // Everything under it goes along, at every depth.
    const gone: StoredBlock[] = [];
    const collect = (parent: StoredBlock): void => {
      gone.push(parent);
      for (const child of this.world.childrenOf(parent.id)) collect(child);
    };
    collect(block);
    if (block.type === 'comment' && !mayRemoveComments(this.#roleOn(boardId), this.userId, gone)) {
      fail(403, "access denied to delete other users' comments");
    }
    for (const each of gone) this.world.blocks.delete(each.id);
    this.world.deletedBlocks.set(blockId, { boardId, blocks: gone, ...deletion(this) });
    this.world.tellBoard(boardId, {
      kind: 'blocksDeleted',
      boardId,
      blockIds: gone.map((each) => each.id),
    });
  }

  async restoreBlock(boardId: string, blockId: string): Promise<void> {
    await this.#enter('restoreBlock');
    const deleted = this.world.deletedBlocks.get(blockId);
    if (deleted?.boardId !== boardId) fail(404, 'no such block in the trash');
    // A comment at the top of the deletion comes back by the comment's rule, anything else by an editor's.
    const top = deleted.blocks[0];
    if (top?.type === 'comment') {
      if (!mayRemoveComments(this.#memberRole(boardId), this.userId, deleted.blocks)) {
        fail(403, "access denied to undelete other users' comments");
      }
    } else mayManageCards(this.world, this.userId, boardId, 'access denied to undelete block');
    const parent = top?.parentId;
    // A block comes back only under a living parent (or the board itself).
    if (parent !== boardId && parent !== undefined && !this.world.blocks.has(parent)) {
      fail(404, 'the parent of the block is gone');
    }
    this.#board(boardId);
    this.world.deletedBlocks.delete(blockId);
    for (const block of deleted.blocks) {
      // As the server: whoever restores is the last to change each block, now.
      this.#touch(block);
      this.world.blocks.set(block.id, block);
    }
    this.#tellBlocks(boardId, deleted.blocks);
  }

  async deletedCards(boardId: string, paging?: Paging): Promise<DeletedItem[]> {
    await this.#enter('deletedCards');
    mayManageCards(this.world, this.userId, boardId, 'access denied to the trash of the board');
    return deletedCardsOf(this.world, boardId, paging);
  }

  // -------------------------------------------------------------- files

  async upload(boardId: string, file: File, options: UploadOptions = {}): Promise<UploadedFile> {
    await this.#enter('upload', options.signal);
    this.#board(boardId);
    if (file.size > LIMITS.fileBytes) {
      fail(413, 'the file is too large', { limit: LIMITS.fileBytes });
    }
    // In parts, as the server takes a large file; progress is told after each.
    const parts = Math.max(1, Math.ceil(file.size / LIMITS.uploadPartBytes));
    for (let part = 1; part <= parts; part += 1) {
      await this.#enter('upload', options.signal);
      options.onProgress?.({
        sentBytes: Math.min(file.size, part * LIMITS.uploadPartBytes),
        totalBytes: file.size,
      });
    }
    const stored = {
      fileId: this.world.newId(),
      boardId,
      name: file.name,
      size: file.size,
      contentType: file.type || 'application/octet-stream',
      blob: file as Blob,
    };
    this.world.files.set(stored.fileId, stored);
    const { blob: _blob, boardId: _boardId, ...answer } = stored;
    return answer;
  }

  async fileBytes(boardId: string, fileId: string): Promise<Blob> {
    await this.#enter('fileBytes');
    const file = this.world.files.get(fileId);
    if (file?.boardId !== boardId || !file.blob) fail(404, 'no such file');
    return file.blob;
  }

  fileUrl(boardId: string, fileId: string): string | undefined {
    const file = this.world.files.get(fileId);
    if (file?.boardId !== boardId || !file.blob) return undefined;
    file.url ??= URL.createObjectURL(file.blob);
    return file.url;
  }

  // ------------------------------------------------------------ changes

  watchBoard(boardId: string, listener: ChangeListener): () => void {
    return this.world.watch({ boardId, userId: this.userId, listener });
  }

  watchSelf(listener: ChangeListener): () => void {
    return this.world.watch({ boardId: undefined, userId: this.userId, listener });
  }
}
