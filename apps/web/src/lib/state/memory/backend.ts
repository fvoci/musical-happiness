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
import type { FileDetails } from '../../editor/files.ts';
import type { DocNode } from '../../editor/schema.ts';
import { readDisplaySettings, type DisplaySettings } from '../../logic/display.ts';
import { isTimeZone, readRegionalSettings } from '../../logic/regional.ts';
import { DEFAULT_ACCOUNT_SETTINGS } from '../../logic/account-settings.ts';
import type { NameChange } from '../../logic/person-names.ts';
import { readLanguageSettings } from '../../logic/language.ts';
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
import { applyContainerPatch, applyCardPatch, applyViewPatch } from '../patches.ts';

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
import { copiedBlocks, giveCopiesFiles } from './copies.ts';
import { memberRole, memberFileUrl } from './container-access.ts';
import { readableFile, storeUpload } from './files.ts';
import { userFound, usersAmong, usersNamed } from './users.ts';
import { changeName } from './account.ts';
import { archiveLines, archiveOf, blockOfLine } from './archive.ts';
import { checkedBody, checkedComment, checkedWhole, flushCard, openMemoryRoom } from './bodies.ts';
import { checkRelations, MemoryReferences, renameCopied } from './references.ts';
import { MemoryContentSearch } from './content-search.ts';
import { searchBoardsOf } from './search.ts';
import { fail } from './refuse.ts';
import { nextScreenState } from './screen-state.ts';
import {
  deletedContainersOf,
  deletedCardsOf,
  deletion,
  mayManageCards,
  placesOf,
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
  readonly bodyVersions: MemoryBodyVersions;
  readonly search: MemoryContentSearch;
  readonly references = new MemoryReferences(this, () => this.#enter('references'));
  /** Milliseconds every call waits before it answers. */
  latency = 0;
  readonly #failures = new Map<MethodName, BackendError>();

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
    await pause(this.latency);
    if (signal?.aborted) throw new BackendError(0, { detail: 'the call was given up' });
    const failure = this.#failures.get(method);
    if (failure) {
      this.#failures.delete(method);
      throw failure;
    }
  }

  /** A board of the `kind`: the server answers a wiki to an operation of cards as one not there. */
  #container(containerId: string, kind: ContainerKind | 'any' = 'any'): Container {
    const board = this.world.containers.get(containerId);
    return board && (kind === 'any' || (board.kind ?? 'board') === kind)
      ? board
      : fail(404, 'no board');
  }

  #block(boardId: string, blockId: string, type?: StoredBlock['type']): StoredBlock {
    const block = this.world.blocks.get(blockId);
    if (block?.containerId !== boardId || block.type === 'page' || (type && block.type !== type)) {
      fail(404, `no such ${type ?? 'block'}`);
    }
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

  async workspaces(): Promise<WorkspaceInfo[]> {
    await this.#enter('workspaces');
    return [copy(this.world.workspace)];
  }

  async users(workspaceId: string): Promise<User[]> {
    await this.#enterWorkspace('users', workspaceId);
    return usersAmong(this.world);
  }

  /** Names visible members and authors, without offering departed authors among people. */
  async usersById(workspaceId: string, userIds: string[]): Promise<User[]> {
    await this.#enterWorkspace('usersById', workspaceId);
    return usersNamed(this.world, this.userId, userIds);
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
    const current = this.world.screenState.get(this.userId) ?? {};
    this.world.screenState.set(this.userId, nextScreenState(current, updated, deleted));
  }

  // ------------------------------------------------------------- boards

  async boards(workspaceId: string): Promise<Container[]> {
    await this.#enterWorkspace('boards', workspaceId);
    const mine = [...this.world.containers.values()].filter(
      (board) => board.kind !== 'wiki' && this.#memberIds(board.id).includes(this.userId),
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
    this.#fileAsNew(this.userId, board);
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
    const role = this.#memberRole(boardId);
    if (role !== 'admin' && role !== 'editor') {
      fail(403, 'access denied to modifying board properties');
    }
    const board = inPropertyOrder(this.#container(boardId), propertyIds);
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

  /** As the server: the workspace's templates, then the built-in ones, each the oldest first. */
  async templates(workspaceId: string): Promise<Container[]> {
    await this.#enterWorkspace('templates', workspaceId);
    const all = [...this.world.containers.values()]
      .filter((board) => board.isTemplate)
      .sort((first, second) => first.createAt - second.createAt);
    return [
      ...all.filter((board) => board.isBuiltin !== true),
      ...all.filter((board) => board.isBuiltin === true),
    ].map(copy);
  }

  /** Into the workspace `to`, as the server's `toWorkspace`. */
  async duplicateContainer(id: string, asTemplate: boolean, to: string): Promise<Container> {
    await this.#enterWorkspace('duplicateContainer', to);
    const source = this.#container(id);
    if (source.kind === 'wiki' && asTemplate) fail(400, 'a wiki is no template');
    return this.#copyContainer(source, asTemplate);
  }

  /** As the server: a copy of the built-in welcome template, without its mark. */
  async onboard(workspaceId: string): Promise<string> {
    await this.#enterWorkspace('onboard', workspaceId);
    const welcome =
      [...this.world.containers.values()].find(
        (board) =>
          board.isTemplate && board.isBuiltin === true && board.properties.onboarding !== undefined,
      ) ?? fail(404, 'no welcome board');
    const { onboarding: _onboarding, ...properties } = welcome.properties;
    const blocks = copiedBlocks(this.world, welcome);
    return this.#insertContainer({ ...welcome, isTemplate: false, properties }, blocks).id;
  }

  /** A copy of a board or a wiki with its blocks but the comments, every identifier new. */
  #copyContainer(source: Container, asTemplate: boolean): Container {
    // The server's titles: a copy of the same kind is "… copy", a template
    // made from a board "New board template", a board from a template keeps it.
    const title =
      source.isTemplate === asTemplate
        ? `${source.title} copy`
        : asTemplate
          ? 'New board template'
          : source.title;
    const blocks = copiedBlocks(this.world, source);
    return this.#insertContainer({ ...source, isTemplate: asTemplate, title }, blocks);
  }

  /**
   * Stores a board and its blocks under new identifiers, the reader its
   * admin. As on the server, only the references it renames follow: a
   * block's board and parent, and what `renameCopied` renames.
   */
  #insertContainer(source: Container, blocks: readonly StoredBlock[]): Container {
    const now = this.world.now();
    const ids = new Map<string, string>([[source.id, this.world.newId()]]);
    for (const block of blocks) ids.set(block.id, this.world.newId());
    const renamed = (id: string): string => ids.get(id) ?? id;
    const cardIds = new Set(blocks.filter((block) => block.type === 'card').map(({ id }) => id));
    const board: Container = {
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
    this.world.containers.set(board.id, board);
    this.world.members.set(board.id, [memberOf(board.id, this.userId, 'admin')]);
    this.#fileAsNew(this.userId, board);
    for (const block of blocks) {
      const made = copy(block);
      renameCopied(source, made, ids, cardIds);
      this.world.blocks.set(renamed(block.id), {
        ...made,
        id: renamed(block.id),
        containerId: board.id,
        parentId: renamed(made.parentId),
        createAt: now,
        updateAt: now,
      });
    }
    giveCopiesFiles(this.world, source.id, board.id);
    this.#tellContainer(board);
    return copy(board);
  }

  // ----------------------------------------------------------- archives

  /**
   * An archive of the memory backend: lines of JSON, as the server's
   * `board.jsonl` is, but not its ZIP. Only this backend reads it back.
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
    if (workspaceId !== this.world.workspace.id) fail(404, 'no such workspace');
    // As the server: the reader's boards and wikis, and the workspace's open ones.
    const boards = [...this.world.containers.values()].filter(
      (board) =>
        !board.isTemplate &&
        (board.type === 'O' || this.#memberIds(board.id).includes(this.userId)),
    );
    return archiveOf(this.world, boards, options);
  }

  async importArchive(workspaceId: string, file: File, options?: UploadOptions) {
    await this.#enter('importArchive', options?.signal);
    if (workspaceId !== this.world.workspace.id) fail(404, 'no such workspace');
    const lines = await archiveLines(file);
    options?.onProgress?.({ sentBytes: file.size, totalBytes: file.size });
    const boards = lines
      .filter((line) => line.type === 'board')
      .map((line) => line.data as Container);
    const blocks = lines
      .filter((line) => line.type === 'block')
      .map((line) => blockOfLine(line.data));
    for (const board of boards) {
      this.#insertContainer(
        board,
        blocks.filter((block) => block.containerId === board.id),
      );
    }
    // Its archives carry no members: nobody is left out.
    return { membersLeftOut: 0 };
  }

  async joinContainer(id: string): Promise<ContainerMember> {
    await this.#enter('joinContainer');
    const board = this.#container(id);
    if (board.isTemplate) fail(403, 'a template cannot be joined');
    if (board.type !== 'O') fail(404, 'no such open board');
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
    this.#container(id);
    return scramble(this.world.members.get(id) ?? []).map(copy);
  }

  async addMember(id: string, userId: string, role: ContainerRole): Promise<ContainerMember> {
    await this.#enter('addMember');
    this.#container(id);
    if (!this.world.users.has(userId)) fail(400, 'no such user', { field: 'userId' });
    const members = this.world.members.get(id) ?? [];
    const existing = members.find((member) => member.userId === userId);
    // As the server: somebody who is a member already keeps their role.
    if (existing) return copy(existing);
    const member = memberOf(id, userId, role);
    this.#setMembers(id, [...members, member]);
    this.#fileAsNew(userId, this.#container(id));
    this.world.tellUsers([userId], { kind: 'container', container: copy(this.#container(id)) });
    return copy(member);
  }

  async setMemberRole(id: string, userId: string, role: ContainerRole): Promise<ContainerMember> {
    await this.#enter('setMemberRole');
    this.#container(id);
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
    this.#container(id);
    const members = this.world.members.get(id) ?? [];
    const next = members.filter((member) => member.userId !== userId);
    if (next.length === members.length) return;
    this.#keepAnAdmin(next);
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
    this.#container(id);
    return copy(this.world.sharing.get(id) ?? { enabled: false, token: '' });
  }

  async setSharing(id: string, change: SharingChange): Promise<Sharing> {
    await this.#enter('setSharing');
    this.#container(id);
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
  #fileAsNew(userId: string, container: Container): void {
    if (container.isTemplate) return;
    const theirs = this.world.categories.get(userId) ?? [];
    const fallback = theirs.find((category) => category.type === 'system');
    if (!fallback || theirs.some(({ containerIds }) => containerIds.includes(container.id))) return;
    fallback.containerIds.push(container.id);
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
      containerIds: [],
      hiddenContainerIds: [],
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
    fallback?.containerIds.push(...category.containerIds);
    fallback?.hiddenContainerIds.push(...category.hiddenContainerIds);
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

  async moveContainerToCategory(
    workspaceId: string,
    id: string,
    categoryId: string,
    index: number,
  ): Promise<void> {
    await this.#enterWorkspace('moveContainerToCategory', workspaceId);
    this.#container(id);
    const target = this.#category(categoryId);
    for (const category of this.#categories()) {
      category.containerIds = category.containerIds.filter((other) => other !== id);
    }
    const at = Math.max(0, Math.min(index, target.containerIds.length));
    target.containerIds.splice(at, 0, id);
    this.#tellCategories();
  }

  async setContainerHidden(
    workspaceId: string,
    categoryId: string,
    id: string,
    hidden: boolean,
  ): Promise<void> {
    await this.#enterWorkspace('setContainerHidden', workspaceId);
    const category = this.#category(categoryId);
    const others = category.hiddenContainerIds.filter((other) => other !== id);
    // As the server: a board the category does not hold is no error.
    category.hiddenContainerIds =
      hidden && category.containerIds.includes(id) ? [...others, id] : others;
    this.#tellCategories();
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
    const block = this.#newBlock(boardId, '', 'card');
    block.title = draft.title ?? '';
    block.fields = {
      icon: draft.icon ?? '',
      isTemplate: draft.isTemplate ?? false,
      properties: copy(draft.properties ?? {}),
    };
    block.body = checkedBody(this.world, boardId, draft.body ?? null);
    checkRelations(this.world, board, block, undefined, 'properties');
    this.world.blocks.set(block.id, block);
    this.#tellBlocks(boardId, [block]);
    return this.#commented(summaryOf(block as StoredCard));
  }

  async patchCard(boardId: string, cardId: string, patch: CardPatch): Promise<CardSummary> {
    await this.#enter('patchCard');
    const before = this.#block(boardId, cardId, 'card') as StoredCard;
    const card = applyCardPatch(before, patch);
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
    const deleted = this.world.deletedBlocks.get(blockId);
    if (deleted?.containerId !== boardId) fail(404, 'no such block in the trash');
    // A comment at the top of the deletion comes back by the comment's rule, anything else by an editor's.
    const top = deleted.blocks[0];
    const card = top?.type !== 'comment';
    if (card) mayManageCards(this.world, this.userId, boardId, 'access denied to undelete block');
    else if (!mayRemoveComments(this.#memberRole(boardId), this.userId, deleted))
      fail(403, "access denied to undelete other users' comments");
    const parent = top?.parentId;
    // A block comes back only under a living parent, or with none.
    if (parent && !this.world.blocks.has(parent)) {
      fail(404, 'the parent of the block is gone');
    }
    this.#container(boardId);
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

  async upload(id: string, file: File, options: UploadOptions = {}): Promise<UploadedFile> {
    await this.#enter('upload', options.signal);
    this.#container(id);
    return storeUpload(this.world, id, file, options, (signal) => this.#enter('upload', signal));
  }

  async fileBytes(id: string, fileId: string): Promise<Blob> {
    await this.#enter('fileBytes');
    const { blob } = readableFile(this.world, this.userId, id, fileId);
    if (!blob) fail(404, 'no such file');
    return blob;
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
