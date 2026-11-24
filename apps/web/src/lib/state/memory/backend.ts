/**
 * In-memory backend matching the server contract for tests and standalone mode.
 * Lists are scrambled (`scramble`) so screens cannot rely on backend order.
 * Helpers for tests: `as`, `failNext`, `latency`.
 */
import { cardPreviewOf, type CardPreview } from '../../editor/card-preview.ts';
import type { FileDetails } from '../../editor/files.ts';
import type { DocNode } from '../../editor/schema.ts';
import type { NameChange } from '../../logic/person-names.ts';
import {
  createBoard,
  type BlockPatch,
  type Container,
  type ContainerMember,
  type ContainerPatch,
  type ContainerKind,
  type BoardView,
} from '../../logic/wire.ts';
import {
  BackendError,
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
  type CardTelling,
  type Category,
  type CategoryDraft,
  type CategoryPatch,
  type ChangeListener,
  type Comment,
  type CommentVersion,
  type DeletedItem,
  type DownloadedFile,
  type DownloadOptions,
  type ImportOptions,
  type ImportSummary,
  type Paging,
  type Sharing,
  type SharingChange,
  type UserOrganization,
  type WorkspaceInfo,
  type UploadedFile,
  type UploadOptions,
  type User,
  type ViewDraft,
} from '../backend.ts';
import { applyContainerPatch, applyCardPatch, applyViewPatch } from '../patches.ts';
import { BlockTurns } from '../write.ts';

import {
  noteChange,
  versionsOf,
  lastCommentAt,
  mayChangeComment,
  mayRemoveComments,
  roleOn,
  type Role,
} from './comments.ts';
import { MemoryBodyVersions, writtenWhole } from './body-versions.ts';
import { pause } from './pause.ts';
import { MemoryPresence } from './presence.ts';
import { MemoryWikis } from './wikis.ts';
import { MemoryDiaries } from './diaries.ts';
import { MemoryCourses } from './courses.ts';
import { purgeMemberExamScores } from './course-exams.ts';
import { checkDiariesComing } from './diary-places.ts';
import { MemoryCategories, fileAsNew } from './categories.ts';
import * as spaces from './spaces.ts';
import { copiedBlocks, copyTitle, insertCopy } from './copies.ts';
import { memberRole, memberFileUrl } from './container-access.ts';
import { MemoryFileVersions } from './file-versions.ts';
import { readableBlob, readableFile, storeUpload } from './files.ts';
import { userFound, usersAmong, usersNamed } from './users.ts';
import { changeName, keepSettings, settingsOf } from './account.ts';
import { archiveOf, exportableContainers, readImportFile, summarizeImport } from './archive.ts';
import { checkedComment, checkedWhole, flushCard, openMemoryRoom } from './bodies.ts';
import { checkRelations, MemoryReferences } from './references.ts';
import { checkArchiveCards, checkCardParent, storeNewCard } from './card-hierarchy.ts';
import { MemoryContentSearch } from './content-search.ts';
import { MemoryNotifications, noteComment } from './notifications.ts';
import { MemoryTime } from './time.ts';
import { searchBoardsOf } from './search.ts';
import { visibleTemplates, welcomeTemplate } from './seed-templates.ts';
import { fail } from './refuse.ts';
import { nextScreenState } from './screen-state.ts';
import {
  deletedContainersOf,
  deletedCardsOf,
  deletion,
  mayManageCards,
  placesOf,
  subtreeOf,
  takeBlockBack,
  takeContainerBack,
} from './trash.ts';
import type { StoredBlock, StoredCard, StoredView, World } from './world.ts';

import {
  attachmentOf,
  commentOf,
  copy,
  detailOf,
  inPropertyOrder,
  LIMITS,
  memberOf,
  scramble,
  summaryOf,
  viewOf,
  wireOf,
} from './shapes.ts';

export { LIMITS, memberOf };

type MethodName = keyof Backend | 'writeCardBody';

export class MemoryBackend implements Backend {
  readonly world: World;
  readonly userId: string;
  readonly presence: MemoryPresence;
  readonly wiki = new MemoryWikis(this);
  readonly diary = new MemoryDiaries(this);
  readonly course = new MemoryCourses(this);
  readonly bodyVersions: MemoryBodyVersions;
  readonly search: MemoryContentSearch;
  readonly references = new MemoryReferences(this, () => this.#enter('references'));
  readonly fileVersions = new MemoryFileVersions(this, () => this.#enter('fileVersions'));
  readonly notifications = new MemoryNotifications(this, () => this.#enter('notifications'));
  readonly time = new MemoryTime(this, () => this.#enter('time'));
  readonly #categories = new MemoryCategories(this, (method, workspaceId) =>
    this.#enterWorkspace(method, workspaceId),
  );
  /** Milliseconds every call waits before it answers. */
  latency = 0;
  readonly #failures = new Map<MethodName, BackendError>();
  readonly #viewTurns = new BlockTurns();

  constructor(world: World, userId: string) {
    this.world = world;
    this.userId = userId;
    this.presence = new MemoryPresence(world, userId);
    this.bodyVersions = new MemoryBodyVersions(world, userId, () => this.#enter('bodyVersions'));
    this.search = new MemoryContentSearch(world, userId, () => this.#enter('search'));
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

  /** `#enter`, then 404 unless `workspaceId` names a workspace of the reader's (`spaces.ts`). */
  async #enterWorkspace(method: MethodName, workspaceId: string): Promise<void> {
    await this.#enter(method);
    spaces.checkWorkspace(this.world, this.userId, workspaceId);
  }

  /**
   * The wait every call starts with, and the failure a test asked for. A call
   * that was given up while it waited (`signal`) ends here as given up: it
   * does not take the failure meant for the next call.
   */
  async #enter(method: MethodName, signal?: AbortSignal): Promise<void> {
    await pause(this.latency);
    if (signal?.aborted) throw new BackendError(0, { detail: 'the call was given up' });
    const failure = this.#failures.get(method);
    if (failure) {
      this.#failures.delete(method);
      throw failure;
    }
  }

  /** Of `kind` where asked; `shared`: board/wiki/course (not diary). */
  #container(containerId: string, kind: ContainerKind | 'any' | 'shared' = 'any'): Container {
    const board = this.world.containers.get(containerId);
    const found = board?.kind ?? 'board';
    const ok = kind === 'any' || (kind === 'shared' && found !== 'diary') || found === kind;
    return board && ok ? board : fail(404, 'no board');
  }

  #block(boardId: string, blockId: string, type?: StoredBlock['type']): StoredBlock {
    const block = this.world.blocks.get(blockId);
    // Pages, entries, units and chapters are reached by routes of their own, not a board's.
    const own =
      block?.containerId === boardId && !['page', 'entry', 'unit', 'chapter'].includes(block.type);
    if (!block || !own || (type && block.type !== type)) fail(404, `no such ${type ?? 'block'}`);
    return block;
  }

  #roleOn(boardId: string): Role {
    return roleOn(this.world.members.get(boardId) ?? [], this.userId);
  }

  /** The reader's role on the board; one they are no member of is not there for them, unless open (403). */
  #memberRole(boardId: string): Exclude<Role, ''> {
    return memberRole(this.world, this.userId, boardId);
  }

  /**
   * A card as the card routes answer it: with when its last comment was
   * posted. A change message has no such time (`wireOf`), as on the server.
   */
  #commented<T extends CardSummary>(card: T): T {
    return { ...card, lastCommentAt: lastCommentAt(this.world.childrenOf(card.id)) };
  }

  #memberIds(containerId: string): string[] {
    return (this.world.members.get(containerId) ?? []).map((member) => member.userId);
  }

  #touch(block: StoredBlock): void {
    block.updateAt = this.world.now();
    block.modifiedBy = this.userId;
  }

  #tellBlocks(id: string, blocks: readonly StoredBlock[]): void {
    this.world.tellContainer(id, { kind: 'blocks', containerId: id, blocks: blocks.map(wireOf) });
  }

  #tellContainer(board: Container): void {
    this.world.tellContainer(board.id, { kind: 'container', container: copy(board) });
    this.world.tellUsers(this.#memberIds(board.id), { kind: 'container', container: copy(board) });
  }

  #newBlock(boardId: string, parentId: string, type: StoredBlock['type']): StoredBlock {
    const now = this.world.now();
    return {
      id: this.world.newId(),
      containerId: boardId,
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

  async patchMe(patch: NameChange): Promise<User> {
    await this.#enter('patchMe');
    return copy(changeName(this.world, this.userId, patch));
  }

  async confirmPassword(password: string): Promise<void> {
    await this.#enter('confirmPassword');
    spaces.confirmPassword(this.world, this.userId, password);
  }

  async workspaces(): Promise<WorkspaceInfo[]> {
    await this.#enter('workspaces');
    return spaces.workspacesOf(this.world, this.userId);
  }

  async organizations(): Promise<UserOrganization[]> {
    await this.#enter('organizations');
    return spaces.organizationsOf(this.world, this.userId);
  }

  async users(workspaceId: string): Promise<User[]> {
    await this.#enterWorkspace('users', workspaceId);
    return usersAmong(this.world, spaces.workersOf(this.world, workspaceId));
  }

  /** Names visible members and authors, without offering departed authors among people. */
  async usersById(workspaceId: string, userIds: string[]): Promise<User[]> {
    await this.#enterWorkspace('usersById', workspaceId);
    return usersNamed(this.world, this.userId, workspaceId, userIds);
  }

  // ----------------------------------------------------------- settings

  async settings(): Promise<AccountSettings> {
    await this.#enter('settings');
    return copy(settingsOf(this.world, this.userId));
  }

  async patchSettings(patch: AccountSettingsPatch): Promise<AccountSettings> {
    await this.#enter('patchSettings');
    return copy(keepSettings(this.world, this.userId, patch));
  }

  async screenState(): Promise<Record<string, string>> {
    await this.#enter('screenState');
    return { ...this.world.screenState.get(this.userId) };
  }

  async patchScreenState(updated: Record<string, string>, deleted: string[] = []): Promise<void> {
    await this.#enter('patchScreenState');
    const current = this.world.screenState.get(this.userId) ?? {};
    this.world.screenState.set(this.userId, nextScreenState(current, updated, deleted));
  }

  // ------------------------------------------------------------- boards

  async boards(workspaceId: string): Promise<Container[]> {
    await this.#enterWorkspace('boards', workspaceId);
    const mine = [...this.world.containers.values()].filter(
      (board) =>
        board.workspaceId === workspaceId &&
        (board.kind ?? 'board') === 'board' &&
        this.#memberIds(board.id).includes(this.userId),
    );
    return scramble(mine).map(copy);
  }

  async container(id: string): Promise<Container> {
    await this.#enter('container');
    return copy(this.#container(id));
  }

  async createBoard(
    workspaceId: string,
    draft: BoardDraft,
    firstView: ViewDraft,
  ): Promise<{ board: Container; view: BoardView }> {
    await this.#enterWorkspace('createBoard', workspaceId);
    const now = this.world.now();
    const board: Container = {
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
    this.world.containers.set(board.id, board);
    this.world.members.set(board.id, [memberOf(board.id, this.userId, 'admin')]);
    fileAsNew(this.world, this.userId, board);
    const view = this.#newBlock(board.id, '', 'view');
    view.title = firstView.title;
    view.fields = copy(firstView.fields);
    this.world.blocks.set(view.id, view);
    this.#tellContainer(board);
    this.#tellBlocks(board.id, [view]);
    return { board: copy(board), view: viewOf(view) };
  }

  async patchContainer(id: string, patch: ContainerPatch): Promise<Container> {
    await this.#enter('patchContainer');
    const board = applyContainerPatch(this.#container(id), patch);
    this.world.containers.set(id, board);
    board.updateAt = this.world.now();
    board.modifiedBy = this.userId;
    this.#tellContainer(board);
    return copy(board);
  }

  async reorderCardProperties(boardId: string, propertyIds: string[]): Promise<string[]> {
    await this.#enter('reorderCardProperties');
    const current = this.#container(boardId, 'board');
    if (!['admin', 'editor'].includes(this.#memberRole(boardId))) {
      fail(403, 'access denied to modifying board properties');
    }
    const board = inPropertyOrder(current, propertyIds);
    board.updateAt = this.world.now();
    board.modifiedBy = this.userId;
    this.world.containers.set(boardId, board);
    this.#tellContainer(board);
    return [...propertyIds];
  }

  async deleteContainer(id: string): Promise<void> {
    await this.#enter('deleteContainer');
    const container = this.#container(id);
    const members = this.world.members.get(id) ?? [];
    const blocks = this.world.blocksOf(id);
    const places = placesOf(this.world, id);
    this.world.deletedContainers.set(id, { container, members, blocks, places, ...deletion(this) });
    for (const block of blocks) this.world.blocks.delete(block.id);
    this.world.containers.delete(id);
    this.world.members.delete(id);
    for (const categories of this.world.categories.values()) {
      for (const category of categories) {
        category.containerIds = category.containerIds.filter((other) => other !== id);
        category.hiddenContainerIds = category.hiddenContainerIds.filter((other) => other !== id);
      }
    }
    const userIds = members.map((member) => member.userId);
    this.world.tellContainer(id, { kind: 'containerDeleted', containerId: id });
    this.world.tellUsers(userIds, { kind: 'containerDeleted', containerId: id });
  }

  /** As the server: for who was its admin; filed again where each person had it. */
  async restoreContainer(id: string): Promise<Container> {
    await this.#enter('restoreContainer');
    const board = takeContainerBack(this.world, this.userId, id);
    this.#tellContainer(board);
    return copy(board);
  }

  async deletedContainers(workspaceId: string, paging?: Paging): Promise<DeletedItem[]> {
    await this.#enterWorkspace('deletedContainers', workspaceId);
    return deletedContainersOf(this.world, this.userId, workspaceId, paging);
  }

  /** As the server matches (`searchBoardsOf`): among the boards this backend lists. */
  async searchBoards(workspaceId: string, query: string): Promise<Container[]> {
    await this.#enterWorkspace('searchBoards', workspaceId);
    return searchBoardsOf(this.world, this.userId, workspaceId, query);
  }

  /** The workspace's templates, then the built-in ones in the reader's language, each oldest first. */
  async templates(workspaceId: string): Promise<Container[]> {
    await this.#enterWorkspace('templates', workspaceId);
    return visibleTemplates(this.world, this.userId, workspaceId).map(copy);
  }

  /** Into `to`, as the server's `toWorkspace`; no diary, nor across spaces but by export. */
  async duplicateContainer(id: string, asTemplate: boolean, to: string): Promise<Container> {
    await this.#enterWorkspace('duplicateContainer', to);
    const source = this.#container(id);
    if (source.kind === 'diary') fail(404, 'no board');
    if (source.kind === 'wiki' && asTemplate) fail(400, 'a wiki is no template');
    if (source.isBuiltin !== true && source.workspaceId !== to) fail(404, 'no board');
    return this.#copyContainer(source, asTemplate, to);
  }

  /** A copy of the marked built-in template in the reader's language, without its template marks. */
  async onboard(workspaceId: string): Promise<string> {
    await this.#enterWorkspace('onboard', workspaceId);
    const welcome = welcomeTemplate(this.world, this.userId) ?? fail(404, 'no welcome board');
    const blocks = copiedBlocks(this.world, welcome);
    return this.#insertContainer({ ...welcome, isTemplate: false }, blocks, workspaceId).id;
  }

  /** A copy of a board or a wiki with its blocks but the comments, every identifier new. */
  #copyContainer(source: Container, asTemplate: boolean, to: string): Container {
    const title = copyTitle(source, asTemplate, settingsOf(this.world, this.userId).language);
    const blocks = copiedBlocks(this.world, source);
    return this.#insertContainer({ ...source, isTemplate: asTemplate, title }, blocks, to);
  }

  /** Stores a copy of a board and its blocks (`insertCopy`), and tells of it. */
  #insertContainer(
    source: Container,
    blocks: readonly StoredBlock[],
    workspaceId: string,
    schedule?: { startDate: string; targetDate: string; studyDays: number },
  ): Container {
    const board = insertCopy(this.world, this.userId, { source, blocks, workspaceId, schedule });
    this.#tellContainer(board);
    return copy(board);
  }

  // ----------------------------------------------------------- archives

  /**
   * An archive of the memory backend: lines of JSON, as the server's
   * `container.jsonl` is, but not its ZIP. Only this backend reads it back.
   */
  async exportContainer(id: string, options?: DownloadOptions): Promise<DownloadedFile> {
    await this.#enter('exportContainer', options?.signal);
    const board = this.#container(id);
    // For its members, as on the server (an open board is readable, its parts are not).
    if (!this.#memberIds(id).includes(this.userId)) fail(403, 'not a member of the board');
    return archiveOf(this.world, [board], options);
  }

  async exportWorkspace(workspaceId: string, options?: DownloadOptions): Promise<DownloadedFile> {
    await this.#enter('exportWorkspace', options?.signal);
    spaces.checkWorkspace(this.world, this.userId, workspaceId);
    return archiveOf(
      this.world,
      exportableContainers(this.world, workspaceId, this.userId),
      options,
    );
  }

  async importArchive(
    workspaceId: string,
    file: File,
    options?: ImportOptions,
  ): Promise<ImportSummary> {
    await this.#enter('importArchive', options?.signal);
    spaces.checkWorkspace(this.world, this.userId, workspaceId);
    const format = options?.format ?? 'fvoci';
    const { containers, blocks, schedules, skipped } = await readImportFile(
      file,
      format,
      workspaceId,
    );
    options?.onProgress?.({ sentBytes: file.size, totalBytes: file.size });
    checkDiariesComing(this.world, this.userId, workspaceId, { containers, blocks });
    checkArchiveCards(containers, blocks);
    const insertedBlocks: StoredBlock[] = [];
    const created = containers.map((board) => {
      const own = blocks.filter((b) => b.containerId === board.id);
      insertedBlocks.push(...own);
      return this.#insertContainer(board, own, workspaceId, schedules.get(board.id));
    });
    return summarizeImport(format, created, insertedBlocks, skipped);
  }

  async joinContainer(id: string): Promise<ContainerMember> {
    await this.#enter('joinContainer');
    const board = this.#container(id);
    if (board.isTemplate) fail(403, 'a template cannot be joined');
    const works = spaces.workersOf(this.world, board.workspaceId).has(this.userId);
    if (board.type !== 'O' || !works) fail(404, 'no such open board');
    const members = this.world.members.get(id) ?? [];
    const existing = members.find((member) => member.userId === this.userId);
    if (existing) return copy(existing);
    const role = board.minimumRole === 'admin' ? 'editor' : board.minimumRole || 'editor';
    const member = memberOf(id, this.userId, role);
    this.#setMembers(id, [...members, member]);
    return copy(member);
  }

  async members(id: string): Promise<ContainerMember[]> {
    await this.#enter('members');
    this.#container(id, 'shared');
    return scramble(this.world.members.get(id) ?? []).map(copy);
  }

  async addMember(id: string, userId: string, role: ContainerRole): Promise<ContainerMember> {
    await this.#enter('addMember');
    const into = this.#container(id, 'shared').workspaceId;
    if (!spaces.workersOf(this.world, into).has(userId))
      fail(400, 'no such user', { field: 'userId' });
    const members = this.world.members.get(id) ?? [];
    const existing = members.find((member) => member.userId === userId);
    // As the server: somebody who is a member already keeps their role.
    if (existing) return copy(existing);
    const member = memberOf(id, userId, role);
    this.#setMembers(id, [...members, member]);
    fileAsNew(this.world, userId, this.#container(id));
    this.world.tellUsers([userId], { kind: 'container', container: copy(this.#container(id)) });
    return copy(member);
  }

  async setMemberRole(id: string, userId: string, role: ContainerRole): Promise<ContainerMember> {
    await this.#enter('setMemberRole');
    this.#container(id, 'shared');
    const members = this.world.members.get(id) ?? [];
    if (!members.some((member) => member.userId === userId)) fail(404, 'no such member');
    const member = memberOf(id, userId, role);
    const next = members.map((one) => (one.userId === userId ? member : one));
    this.#keepAnAdmin(next);
    this.#setMembers(id, next);
    return copy(member);
  }

  async removeMember(id: string, userId: string): Promise<void> {
    await this.#enter('removeMember');
    this.#container(id, 'shared');
    const members = this.world.members.get(id) ?? [];
    const next = members.filter((member) => member.userId !== userId);
    if (next.length === members.length) return;
    this.#keepAnAdmin(next);
    purgeMemberExamScores(this.world, id, userId);
    this.#setMembers(id, next);
    this.world.tellGone(userId, id);
  }

  /** A board keeps an admin, as on the server (409, `last_admin`). */
  #keepAnAdmin(members: readonly ContainerMember[]): void {
    if (!members.some((member) => member.schemeAdmin)) {
      fail(409, 'a board keeps its last admin', { reason: 'last_admin' });
    }
  }

  #setMembers(id: string, members: ContainerMember[]): void {
    this.world.members.set(id, members);
    this.world.tellContainer(id, { kind: 'members', containerId: id, members: copy(members) });
  }

  async sharing(id: string): Promise<Sharing> {
    await this.#enter('sharing');
    this.#container(id, 'shared');
    return copy(this.world.sharing.get(id) ?? { enabled: false, token: '' });
  }

  async setSharing(id: string, change: SharingChange): Promise<Sharing> {
    await this.#enter('setSharing');
    this.#container(id, 'shared');
    const before = this.world.sharing.get(id);
    const token =
      !before?.token || change.regenerate ? this.world.newId().replaceAll('-', '') : before.token;
    const sharing = { enabled: change.enabled, token };
    this.world.sharing.set(id, sharing);
    return copy(sharing);
  }

  /** As the server: by the login name or the name, whatever the case, the oldest first, ten at most. */
  async searchUsers(workspaceId: string, query: string): Promise<User[]> {
    await this.#enter('searchUsers');
    const wanted = query.trim().toLowerCase();
    return (await this.users(workspaceId))
      .filter((user) => userFound(user, wanted))
      .sort((first, second) => first.createAt - second.createAt)
      .slice(0, 10);
  }

  // --------------------------------------------------------- categories

  categories(workspaceId: string): Promise<Category[]> {
    return this.#categories.categories(workspaceId);
  }

  createCategory(workspaceId: string, draft: CategoryDraft): Promise<Category> {
    return this.#categories.createCategory(workspaceId, draft);
  }

  patchCategory(workspaceId: string, categoryId: string, patch: CategoryPatch): Promise<Category> {
    return this.#categories.patchCategory(workspaceId, categoryId, patch);
  }

  deleteCategory(workspaceId: string, categoryId: string): Promise<void> {
    return this.#categories.deleteCategory(workspaceId, categoryId);
  }

  reorderCategories(workspaceId: string, categoryIds: string[]): Promise<void> {
    return this.#categories.reorderCategories(workspaceId, categoryIds);
  }

  moveContainerToCategory(
    workspaceId: string,
    id: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
    return this.#categories.moveContainerToCategory(workspaceId, id, categoryId, index);
  }

  setContainerHidden(
    workspaceId: string,
    categoryId: string,
    id: string,
    hidden: boolean,
  ): Promise<void> {
    return this.#categories.setContainerHidden(workspaceId, categoryId, id, hidden);
  }

  // -------------------------------------------------------------- views

  async views(boardId: string): Promise<BoardView[]> {
    await this.#enter('views');
    this.#memberRole(boardId);
    return scramble(this.world.blocksOf(boardId).filter((block) => block.type === 'view')).map(
      viewOf,
    );
  }

  async createView(boardId: string, draft: ViewDraft): Promise<BoardView> {
    await this.#enter('createView');
    this.#container(boardId, 'board');
    const block = this.#newBlock(boardId, '', 'view');
    block.title = draft.title;
    block.fields = copy(draft.fields);
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    return viewOf(block);
  }

  patchView(boardId: string, viewId: string, patch: BlockPatch): Promise<BoardView> {
    return this.#viewTurns.patchView(viewId, patch, async (sent) => {
      await this.#enter('patchView');
      const stored = this.#block(boardId, viewId, 'view') as StoredView;
      if (sent.readAt !== undefined && stored.updateAt !== sent.readAt) {
        fail(409, 'the block has changed since it was read');
      }
      const view: StoredView = { ...stored, ...applyViewPatch(stored, sent) };
      this.world.blocks.set(viewId, view);
      this.#touch(view);
      this.#tellBlocks(boardId, [view]);
      return { before: viewOf(stored), after: viewOf(view) };
    });
  }

  // -------------------------------------------------------------- cards

  async cards(boardId: string): Promise<CardSummary[]> {
    await this.#enter('cards');
    this.#memberRole(boardId);
    const cards = this.world.blocksOf(boardId).filter((block) => block.type === 'card');
    return scramble(cards).map((card) => this.#commented(summaryOf(card as StoredCard)));
  }

  async cardPreviews(boardId: string): Promise<Map<string, CardPreview>> {
    await this.#enter('cardPreviews');
    this.#memberRole(boardId);
    const cards = this.world.blocksOf(boardId).filter((block) => block.type === 'card');
    return new Map(cards.map((card) => [card.id, cardPreviewOf(card.body)]));
  }

  async card(boardId: string, cardId: string): Promise<CardDetail> {
    await this.#enter('card');
    this.#memberRole(boardId);
    const card = flushCard(this.world, this.#block(boardId, cardId, 'card') as StoredCard);
    return this.#commented(detailOf(card));
  }

  async createCard(boardId: string, draft: CardDraft): Promise<CardSummary> {
    await this.#enter('createCard');
    const board = this.#container(boardId, 'board');
    const block = this.#newBlock(boardId, draft.parentId ?? '', 'card');
    const stored = storeNewCard(this.world, board, block, draft);
    this.#tellBlocks(boardId, [stored]);
    return this.#commented(summaryOf(stored));
  }

  /** Tells nobody of a change of a card, quiet or not: only comments are told here. */
  async patchCard(
    boardId: string,
    cardId: string,
    patch: CardPatch,
    _telling?: CardTelling,
  ): Promise<CardSummary> {
    await this.#enter('patchCard');
    const before = this.#block(boardId, cardId, 'card') as StoredCard;
    const card = applyCardPatch(before, patch);
    checkCardParent(this.world, boardId, card, card.parentId, false, before);
    checkRelations(this.world, this.#container(boardId), card, before, 'updatedProperties');
    this.world.blocks.set(cardId, card);
    this.#touch(card);
    this.#tellBlocks(boardId, [card]);
    return this.#commented(summaryOf(card));
  }

  /** A body written whole by another client of the server's API; its room gets a new document. */
  async writeCardBody(boardId: string, cardId: string, write: BodyWrite): Promise<CardDetail> {
    await this.#enter('writeCardBody');
    const card = this.#block(boardId, cardId, 'card') as StoredCard;
    const body = checkedWhole(this.world, boardId, cardId, write.body);
    if (card.updateAt !== write.readAt) fail(409, 'the card changed since it was read');
    writtenWhole(this.world, card, body, this.userId, () => {
      card.body = body;
      this.#touch(card);
    });
    this.world.bodyWritten?.(cardId);
    this.#tellBlocks(boardId, [card]);
    return this.#commented(detailOf(card));
  }

  // ------------------------------------------- comments and attachments

  async comments(boardId: string, cardId: string): Promise<Comment[]> {
    await this.#enter('comments');
    this.#memberRole(boardId);
    this.#block(boardId, cardId, 'card');
    return scramble(this.world.childrenOf(cardId).filter((block) => block.type === 'comment')).map(
      commentOf,
    );
  }

  async createComment(boardId: string, cardId: string, body: DocNode): Promise<Comment> {
    await this.#enter('createComment');
    this.#block(boardId, cardId, 'card');
    const checked = checkedComment(this.world, boardId, body);
    if (checked === null) fail(400, 'a comment needs a text', { field: 'body' });
    const block = this.#newBlock(boardId, cardId, 'comment');
    block.body = checked;
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    noteComment(this.world, block);
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
    const checked = checkedComment(this.world, boardId, write.body);
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
    this.#memberRole(boardId);
    this.#block(boardId, cardId, 'card');
    return scramble(
      this.world.childrenOf(cardId).filter((block) => block.type === 'attachment'),
    ).map(attachmentOf);
  }

  async createAttachment(boardId: string, cardId: string, file: UploadedFile): Promise<Attachment> {
    await this.#enter('createAttachment');
    this.#block(boardId, cardId, 'card');
    const stored = this.world.files.get(file.fileId);
    if (stored?.containerId !== boardId)
      fail(400, 'no such file on this board', { field: 'fileId' });
    const block = this.#newBlock(boardId, cardId, 'attachment');
    block.title = file.name;
    block.file = {
      fileId: stored.fileId,
      name: file.name,
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
    const gone = subtreeOf(this.world, block);
    const entry = { containerId: boardId, blocks: gone, ...deletion(this) };
    if (block.type === 'comment' && !mayRemoveComments(this.#roleOn(boardId), this.userId, entry)) {
      fail(403, "access denied to delete other users' comments");
    }
    for (const each of gone) this.world.blocks.delete(each.id);
    this.world.deletedBlocks.set(blockId, entry);
    this.world.tellContainer(boardId, {
      kind: 'blocksDeleted',
      containerId: boardId,
      blockIds: gone.map((each) => each.id),
    });
  }

  async restoreBlock(boardId: string, blockId: string): Promise<void> {
    await this.#enter('restoreBlock');
    const restored = takeBlockBack(
      this.world,
      this.userId,
      boardId,
      blockId,
      () => this.#memberRole(boardId),
      () => {
        this.#container(boardId);
      },
    );
    this.#tellBlocks(boardId, restored);
  }

  async deletedCards(boardId: string, paging?: Paging): Promise<DeletedItem[]> {
    await this.#enter('deletedCards');
    mayManageCards(this.world, this.userId, boardId, 'access denied to the trash of the board');
    return deletedCardsOf(this.world, boardId, paging);
  }

  // -------------------------------------------------------------- files

  async upload(id: string, file: File, options: UploadOptions = {}): Promise<UploadedFile> {
    await this.#enter('upload', options.signal);
    this.#container(id);
    return storeUpload(this, id, file, options, (signal) => this.#enter('upload', signal));
  }

  async fileBytes(id: string, fileId: string): Promise<Blob> {
    await this.#enter('fileBytes');
    return readableBlob(this.world, this.userId, id, fileId);
  }

  async fileDetails(id: string, fileId: string): Promise<FileDetails> {
    await this.#enter('fileDetails');
    const { name, size } = readableFile(this.world, this.userId, id, fileId);
    return { name, size };
  }

  fileUrl(id: string, fileId: string): string | undefined {
    return memberFileUrl(this.world, this.userId, id, fileId);
  }

  watchContainer(containerId: string, listener: ChangeListener): () => void {
    return this.world.watch({ containerId, userId: this.userId, listener });
  }

  watchSelf(listener: ChangeListener): () => void {
    return this.world.watch({ containerId: undefined, userId: this.userId, listener });
  }

  openDoc(_workspaceId: string, containerId: string, cardId: string, hears: BodyDocHears): BodyDoc {
    return openMemoryRoom(this.world, this, containerId, cardId, hears);
  }
}
