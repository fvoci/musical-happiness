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
import {
  countTasks,
  MAX_BODY_BYTES,
  storedSize,
  validateDoc,
  type DocNode,
} from '../../editor/schema.ts';
import {
  DEFAULT_DISPLAY_SETTINGS,
  readDisplaySettings,
  type DisplaySettings,
} from '../../logic/display.ts';
import {
  DEFAULT_REGIONAL_SETTINGS,
  isTimeZone,
  readRegionalSettings,
} from '../../logic/regional.ts';
import {
  createBoard,
  createBoardView,
  createCard,
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
  type ErrorStatus,
  type Team,
  type UploadedFile,
  type UploadOptions,
  type User,
  type ViewDraft,
} from '../backend.ts';

import { applyBoardPatch, applyCardPatch, applyViewPatch } from '../patches.ts';

import type { StoredBlock, StoredCard, StoredView, World } from './world.ts';

/** The limits of the server design: section 5.7 (bodies) and 4.2, 5.6 (files). */
export const LIMITS = {
  bodyBytes: MAX_BODY_BYTES,
  fileBytes: 100 * 1024 * 1024,
  uploadPartBytes: 8 * 1024 * 1024,
  screenStateKeys: 100,
  screenStateValueBytes: 4 * 1024,
} as const;

const SCREEN_STATE_KEY = /^[A-Za-z0-9_.-]{1,64}$/;

const copy = <T>(value: T): T => structuredClone(value);

function fail(status: ErrorStatus, message: string, extra: object = {}): never {
  throw new BackendError(status, { message, ...extra });
}

function summaryOf(card: StoredCard): CardSummary {
  const { body, file: _file, ...block } = card;
  const tasks = body ? countTasks(body) : { total: 0, checked: 0 };
  return copy({
    ...createCard(block),
    hasBody: body !== null && body !== undefined,
    tasksTotal: tasks.total,
    tasksDone: tasks.checked,
  });
}

function detailOf(card: StoredCard): CardDetail {
  return { ...summaryOf(card), body: copy(card.body ?? null) };
}

function viewOf(view: StoredBlock): BoardView {
  const { body: _body, file: _file, ...block } = view;
  return copy(createBoardView(block));
}

function commentOf(block: StoredBlock): Comment {
  const { body, file: _file, ...rest } = block;
  return copy({ ...rest, type: 'comment', body: body ?? null });
}

function attachmentOf(block: StoredBlock): Attachment {
  const { body: _body, file, ...rest } = block;
  if (!file) throw new Error(`attachment ${block.id} has no file`);
  return copy({ ...rest, type: 'attachment', ...file });
}

function wireOf(block: StoredBlock): BoardView | CardSummary | Comment | Attachment {
  switch (block.type) {
    case 'view':
      return viewOf(block);
    case 'card':
      return summaryOf(block as StoredCard);
    case 'comment':
      return commentOf(block);
    default:
      return attachmentOf(block);
  }
}

/** A list in an order nobody should rely on: by a hash of the identifiers. */
function scramble<T extends { id: string } | { userId: string }>(items: readonly T[]): T[] {
  const key = (item: T): number => {
    const text = 'id' in item ? item.id : item.userId;
    let hash = 0;
    for (let at = 0; at < text.length; at += 1) hash = (hash * 31 + text.charCodeAt(at)) % 9973;
    return hash;
  };
  return [...items].sort((first, second) => key(first) - key(second));
}

/** The files a body shows. */
function imageFileIds(node: DocNode, found: string[] = []): string[] {
  if (node.type === 'image' && typeof node.attrs?.fileId === 'string')
    found.push(node.attrs.fileId);
  for (const child of node.content ?? []) imageFileIds(child, found);
  return found;
}

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
    if (signal?.aborted) throw new BackendError(0, { message: 'the call was given up' });
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

  async teams(): Promise<Team[]> {
    await this.#enter('teams');
    return [copy(this.world.team)];
  }

  async users(teamId: string): Promise<User[]> {
    await this.#enter('users');
    if (teamId !== this.world.team.id) fail(404, 'no such team');
    return scramble([...this.world.users.values()]).map((user) => {
      const { isAdmin: _isAdmin, ...rest } = copy(user);
      return rest;
    });
  }

  /** As strict as the server: no empty list, no identifier twice, every one a user's. */
  async usersById(teamId: string, userIds: string[]): Promise<User[]> {
    const users = await this.users(teamId);
    if (userIds.length === 0) fail(400, 'no users asked for');
    if (new Set(userIds).size !== userIds.length) fail(404, 'a user asked for twice');
    if (userIds.some((id) => !this.world.users.has(id))) fail(404, 'no such user');
    return users.filter((user) => userIds.includes(user.id));
  }

  // ----------------------------------------------------------- settings

  #settings(): AccountSettings {
    return (
      this.world.settings.get(this.userId) ?? {
        ...DEFAULT_REGIONAL_SETTINGS,
        ...DEFAULT_DISPLAY_SETTINGS,
      }
    );
  }

  async settings(): Promise<AccountSettings> {
    await this.#enter('settings');
    return copy(this.#settings());
  }

  async patchSettings(patch: AccountSettingsPatch): Promise<AccountSettings> {
    await this.#enter('patchSettings');
    const defaults: AccountSettings = { ...DEFAULT_REGIONAL_SETTINGS, ...DEFAULT_DISPLAY_SETTINGS };
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
    const next: AccountSettings = { ...regional, ...display };
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
      if (!SCREEN_STATE_KEY.test(key)) fail(400, 'invalid key', { field: key });
      if (new TextEncoder().encode(value).length > LIMITS.screenStateValueBytes) {
        fail(400, 'value too long', { field: key, limit: LIMITS.screenStateValueBytes });
      }
      next[key] = value;
    }
    if (Object.keys(next).length > LIMITS.screenStateKeys) {
      fail(400, 'too many keys', { limit: LIMITS.screenStateKeys });
    }
    this.world.screenState.set(this.userId, next);
  }

  // ------------------------------------------------------------- boards

  async boards(teamId: string): Promise<Board[]> {
    await this.#enter('boards');
    if (teamId !== this.world.team.id) fail(404, 'no such team');
    const mine = [...this.world.boards.values()].filter((board) =>
      this.#memberIds(board.id).includes(this.userId),
    );
    return scramble(mine).map(copy);
  }

  async board(boardId: string): Promise<Board> {
    await this.#enter('board');
    return copy(this.#board(boardId));
  }

  async createBoard(teamId: string, draft: BoardDraft): Promise<Board> {
    await this.#enter('createBoard');
    if (teamId !== this.world.team.id) fail(404, 'no such team');
    const now = this.world.now();
    const board: Board = {
      ...createBoard(),
      id: this.world.newId(),
      teamId,
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
    this.#tellBoard(board);
    return copy(board);
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

  async deleteBoard(boardId: string): Promise<void> {
    await this.#enter('deleteBoard');
    const board = this.#board(boardId);
    const members = this.world.members.get(boardId) ?? [];
    const blocks = this.world.blocksOf(boardId);
    this.world.deletedBoards.set(boardId, { board, members, blocks });
    for (const block of blocks) this.world.blocks.delete(block.id);
    this.world.boards.delete(boardId);
    this.world.members.delete(boardId);
    for (const categories of this.world.categories.values()) {
      for (const category of categories) {
        category.boardIds = category.boardIds.filter((id) => id !== boardId);
      }
    }
    const userIds = members.map((member) => member.userId);
    this.world.tellBoard(boardId, { kind: 'boardDeleted', boardId });
    this.world.tellUsers(userIds, { kind: 'boardDeleted', boardId });
  }

  async restoreBoard(boardId: string): Promise<Board> {
    await this.#enter('restoreBoard');
    const deleted = this.world.deletedBoards.get(boardId) ?? fail(404, 'no such board');
    this.world.deletedBoards.delete(boardId);
    this.world.boards.set(boardId, deleted.board);
    this.world.members.set(boardId, deleted.members);
    for (const block of deleted.blocks) this.world.blocks.set(block.id, block);
    this.#tellBoard(deleted.board);
    return copy(deleted.board);
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

  #tellCategories(): void {
    this.world.tellUsers([this.userId], {
      kind: 'categories',
      categories: copy(this.#categories()),
    });
  }

  async categories(teamId: string): Promise<Category[]> {
    await this.#enter('categories');
    if (teamId !== this.world.team.id) fail(404, 'no such team');
    return scramble(this.#categories()).map(copy);
  }

  async createCategory(teamId: string, draft: CategoryDraft): Promise<Category> {
    await this.#enter('createCategory');
    if (teamId !== this.world.team.id) fail(404, 'no such team');
    if (draft.name.trim() === '') fail(400, 'a category needs a name', { field: 'name' });
    const now = this.world.now();
    const mine = this.#categories();
    // A new category goes first, as the original's did.
    for (const category of mine) category.sortOrder += 10;
    const category: Category = {
      id: this.world.newId(),
      name: draft.name,
      userId: this.userId,
      teamId,
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

  async patchCategory(categoryId: string, patch: CategoryPatch): Promise<Category> {
    await this.#enter('patchCategory');
    const category = this.#category(categoryId);
    if (patch.name !== undefined) {
      if (patch.name.trim() === '') fail(400, 'a category needs a name', { field: 'name' });
      category.name = patch.name;
    }
    if (patch.collapsed !== undefined) category.collapsed = patch.collapsed;
    category.updateAt = this.world.now();
    this.#tellCategories();
    return copy(category);
  }

  async deleteCategory(categoryId: string): Promise<void> {
    await this.#enter('deleteCategory');
    const category = this.#category(categoryId);
    if (category.type === 'system') fail(400, 'the default category cannot be deleted');
    const mine = this.#categories();
    // Its boards fall back to the default category.
    const fallback = mine.find((candidate) => candidate.type === 'system');
    fallback?.boardIds.push(...category.boardIds);
    mine.splice(mine.indexOf(category), 1);
    this.#tellCategories();
  }

  async reorderCategories(teamId: string, categoryIds: string[]): Promise<void> {
    await this.#enter('reorderCategories');
    const mine = this.#categories();
    const known = new Set(mine.map((category) => category.id));
    if (categoryIds.length !== mine.length || !categoryIds.every((id) => known.has(id))) {
      fail(400, 'the order must name every category once');
    }
    for (const category of mine) category.sortOrder = categoryIds.indexOf(category.id) * 10;
    this.#tellCategories();
  }

  async moveBoardToCategory(boardId: string, categoryId: string, index: number): Promise<void> {
    await this.#enter('moveBoardToCategory');
    this.#board(boardId);
    const target = this.#category(categoryId);
    for (const category of this.#categories()) {
      category.boardIds = category.boardIds.filter((id) => id !== boardId);
    }
    const at = Math.max(0, Math.min(index, target.boardIds.length));
    target.boardIds.splice(at, 0, boardId);
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
    return scramble(cards).map((card) => summaryOf(card as StoredCard));
  }

  async card(boardId: string, cardId: string): Promise<CardDetail> {
    await this.#enter('card');
    return detailOf(this.#block(boardId, cardId, 'card') as StoredCard);
  }

  async createCard(boardId: string, draft: CardDraft): Promise<CardSummary> {
    await this.#enter('createCard');
    this.#board(boardId);
    const block = this.#newBlock(boardId, boardId, 'card');
    block.title = draft.title ?? '';
    block.fields = {
      icon: draft.icon ?? '',
      isTemplate: draft.isTemplate ?? false,
      properties: copy(draft.properties ?? {}),
    };
    block.body = this.#checkBody(boardId, draft.body ?? null);
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    return summaryOf(block as StoredCard);
  }

  async patchCard(boardId: string, cardId: string, patch: CardPatch): Promise<CardSummary> {
    await this.#enter('patchCard');
    const card = applyCardPatch(this.#block(boardId, cardId, 'card') as StoredCard, patch);
    this.world.blocks.set(cardId, card);
    this.#touch(card);
    this.#tellBlocks(boardId, [card]);
    return summaryOf(card);
  }

  async writeCardBody(boardId: string, cardId: string, write: BodyWrite): Promise<CardDetail> {
    await this.#enter('writeCardBody');
    const card = this.#block(boardId, cardId, 'card') as StoredCard;
    const body = this.#checkBody(boardId, write.body);
    if (card.updateAt !== write.readAt) {
      fail(409, 'the card changed since it was read', { reason: 'stale' });
    }
    card.body = body;
    this.#touch(card);
    this.#tellBlocks(boardId, [card]);
    return detailOf(card);
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
    const block = this.#block(boardId, commentId, 'comment');
    const checked = this.#checkBody(boardId, write.body);
    if (checked === null) fail(400, 'a comment needs a text', { field: 'body' });
    if (block.updateAt !== write.readAt) {
      fail(409, 'the comment changed since it was read', { reason: 'stale' });
    }
    block.body = checked;
    this.#touch(block);
    this.#tellBlocks(boardId, [block]);
    return commentOf(block);
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
    for (const each of gone) this.world.blocks.delete(each.id);
    this.world.deletedBlocks.set(blockId, { boardId, blocks: gone });
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
    const parent = deleted.blocks[0]?.parentId;
    // A block comes back only under a living parent (or the board itself).
    if (parent !== boardId && parent !== undefined && !this.world.blocks.has(parent)) {
      fail(404, 'the parent of the block is gone');
    }
    this.#board(boardId);
    this.world.deletedBlocks.delete(blockId);
    for (const block of deleted.blocks) this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, deleted.blocks);
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

export function memberOf(
  boardId: string,
  userId: string,
  role: 'admin' | 'editor' | 'commenter' | 'viewer',
): BoardMember {
  return {
    boardId,
    userId,
    minimumRole: 'editor',
    schemeAdmin: role === 'admin',
    schemeEditor: role === 'editor',
    schemeCommenter: role === 'commenter',
    schemeViewer: role === 'viewer',
    synthetic: false,
  };
}
