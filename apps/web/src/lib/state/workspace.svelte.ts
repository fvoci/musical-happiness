/**
 * The reader's workspace: who they are, the people around them, the boards
 * they can see, their sidebar and their settings. One per app.
 *
 * The reader may work in several workspaces; the one shown is the one the
 * address names (`enter`). What is of a workspace (its people, boards,
 * sidebar, the open board, undo) is read anew when another is entered, and
 * nothing of the one left stays.
 *
 * Screens read the fields and call the methods; the methods talk to the
 * backend. A write shows at once and is taken back if the backend refuses
 * it (see `write.ts`); what went wrong is put into `problems` for the shell
 * to tell the reader.
 */
import { setDisplaySettings } from '../display.svelte.ts';
import { DEFAULT_ACCOUNT_SETTINGS } from '../logic/account-settings.ts';
import { resolveRegional, type Regional } from '../logic/regional.ts';
import { UndoManager } from '../logic/undo.ts';
import type { Container } from '../logic/wire.ts';
import { browserTimeZone, setRegionalSettings } from '../regional.ts';

import {
  isBackendError,
  sessionOver,
  type AccountSettings,
  type AccountSettingsPatch,
  type Backend,
  type BackendError,
  type BoardDraft,
  type Category,
  type Change,
  type DownloadedFile,
  type Imported,
  type WorkspaceInfo,
  type User,
} from './backend.ts';
import { BoardStore } from './board.svelte.ts';
import { hiddenContainersOf, sidebarOf, withoutContainer, type SidebarCategory } from './order.ts';
import { SidebarWrites } from './sidebar-writes.ts';
import { Transfers } from './transfers.svelte.ts';
import { UserNames, type Learning } from './user-names.ts';
import type { WorkspaceSnapshot } from './workspace-cache.ts';
import {
  enterCachedWorkspace,
  refreshWorkspaceSidebar,
  rememberWorkspaceSnapshot,
  WorkspaceCaches,
  type CacheLimits,
} from './workspace-caches.ts';
import { WorkspaceCategories } from './workspace-categories.ts';
import * as entry from './workspace-entry.ts';
import type { LoadStatus, ProblemAction } from './workspace-types.ts';

export type { LoadStatus, ProblemAction };

/** Something a write or a read failed with, for the reader to be told. */
export interface Problem {
  id: number;
  /** What was being done, as a key the shell has a text for. */
  action: ProblemAction;
  error: BackendError | undefined;
}

export interface WorkspaceOptions {
  backend: Backend;
  /** The language the app is shown in, a BCP 47 tag. Decides alphabetical order. */
  locale: () => string;
  /**
   * The workspace this user was last in on this device, if the app keeps
   * one: entered when `load` is not told which.
   */
  lastWorkspace?: (userId: string) => string | undefined;
  /** How somebody whose account was deleted is named, in the app's language (`UserNames`). */
  goneUserName?: () => string;
  /** Optional bounds for the in-memory navigation caches (used in tests). */
  cacheLimits?: CacheLimits;
}

export class Workspace {
  readonly backend: Backend;
  readonly #locale: () => string;
  readonly #lastWorkspace: ((userId: string) => string | undefined) | undefined;

  status = $state<LoadStatus>('idle');
  me = $state.raw<User | undefined>(undefined);
  /** The workspaces the reader works in (not those they are a guest of), in the backend's order. */
  workspaces = $state.raw<WorkspaceInfo[]>([]);
  /** The workspace entered; undefined while none is, or the one asked for is missing or closed. */
  info = $state.raw<WorkspaceInfo | undefined>(undefined);
  /** The workspace asked for, when it is theirs and closed to this session (`sso`, `password`). */
  closed = $state.raw<WorkspaceInfo | undefined>(undefined);
  /** How the workspace last asked for (`asked`) stands. */
  entered = $state<entry.EnterStatus>('none');
  /** The identifier of the workspace last asked for, whether or not it was found. */
  asked = $state<string | undefined>(undefined);
  /** `setFirstPassword` is about to show the session's password: a reading refused meanwhile closes nothing. */
  showing = false;
  users = $state.raw<User[]>([]);
  /** The workspace's list as the backend gives it without a search: on the server, its oldest ten (D2). */
  workspaceUsers = $state.raw<User[]>([]);
  containers = $state.raw<Container[]>([]);
  categories = $state.raw<Category[]>([]);
  settings = $state.raw<AccountSettings>(DEFAULT_ACCOUNT_SETTINGS);
  problems = $state.raw<Problem[]>([]);

  /** The board that is open, if one is. */
  board = $state.raw<BoardStore | undefined>(undefined);

  /** Undo and redo of what the reader did in this visit, and whether one waits (`busy`). */
  readonly undo = new UndoManager();
  canUndo = $state(false);
  canRedo = $state(false);
  undoBusy = $state(false);

  /** The sidebar: categories and their boards, in the order to show them. */
  readonly sidebar = $derived<SidebarCategory[]>(
    sidebarOf(this.categories, this.containers, this.locale),
  );

  /** The boards the reader hid from the sidebar, by title. */
  readonly hiddenContainers = $derived<Container[]>(
    hiddenContainersOf(this.categories, this.containers, this.sidebar, this.locale),
  );

  /** Time zone, first day of the week and way of writing hours, settled for this device. */
  readonly regional = $derived<Regional>(
    resolveRegional(this.settings, this.locale, browserTimeZone()),
  );

  readonly #usersById = $derived<Record<string, User | undefined>>(
    Object.fromEntries(this.users.map((user) => [user.id, user])),
  );
  #stopWatching: (() => void) | undefined;
  #problemCount = 0;
  /** Readings again (`#refresh`) begun: the latest is the one that counts. */
  #refreshing = 0;
  /** Session generation: invalidates in-flight reads after sign-out or reload. */
  #sessionGen = 0;
  /** Visit eviction generation: blocks pre-eviction reads from re-caching the visit. */
  #visitEvictionGen = 0;
  /** Navigation caches: recent workspaces, boards and wiki last-opened pages. */
  readonly #caches: WorkspaceCaches<BoardStore>;
  /** Workspace currently being fetched by `#enter`, so a concurrent `resync` coalesces. */
  #enteringWorkspaceId: string | undefined;
  #pendingResync = false;
  /** Whether the currently shown workspace snapshot is valid to save back into cache on leave. */
  #cacheableVisit = false;

  constructor(options: WorkspaceOptions) {
    this.backend = options.backend;
    this.#locale = options.locale;
    this.#lastWorkspace = options.lastWorkspace;
    this.#caches = new WorkspaceCaches<BoardStore>(options.cacheLimits);
    this.#userNames = new UserNames(this, options.goneUserName);
    this.undo.onStateDidChange = () => {
      this.canUndo = this.undo.canUndo;
      this.canRedo = this.undo.canRedo;
      this.undoBusy = this.undo.busy;
    };
  }

  get locale(): string {
    return this.#locale();
  }

  user(userId: string): User | undefined {
    return this.#usersById[userId];
  }

  /** The name to show for a user (`UserNames.nameOf`), the reader's before any list of people. */
  userName(userId: string): string | undefined {
    const user = this.#usersById[userId] ?? (userId === this.me?.id ? this.me : undefined);
    return user && this.#userNames.nameOf(user);
  }

  /**
   * Reads everything the shell needs: the reader, their workspaces and settings, then the
   * workspace `workspaceId` (`enter`), or without one `firstChoice`. Call again to retry.
   * Where `leaves` says of the settings that the page loads again for them, nothing more is read.
   */
  async load(workspaceId?: string, leaves?: (settings: AccountSettings) => boolean): Promise<void> {
    this.#sessionGen += 1;
    const session = this.#sessionGen;
    this.#entering += 1;
    this.#refreshing += 1;
    this.#caches.clear();
    this.#leave(undefined, false);
    this.workspaces = [];
    this.status = 'loading';
    try {
      const [me, workspaces, settings] = await Promise.all([
        this.backend.me(),
        this.backend.workspaces(),
        this.backend.settings(),
      ]);
      if (session !== this.#sessionGen) return;
      this.me = me;
      this.workspaces = workspaces;
      this.#takeSettings(settings);
      if (leaves?.(settings)) return;
      this.#stopWatching?.();
      this.#stopWatching = this.backend.watchSelf(this.#apply.bind(this));
      const chosen = workspaceId ?? this.firstChoice();
      if (chosen === undefined) this.#leave(undefined, false);
      else await this.#enter(chosen, true);
      if (session !== this.#sessionGen) return;
      this.status = 'ready';
    } catch (error) {
      if (session !== this.#sessionGen) return;
      const over = sessionOver(error);
      if (over) {
        this.#over(over);
        return;
      }
      this.status = 'failed';
      this.report('load', error);
    }
  }

  /** The workspace to start in when the address names none (`firstChoiceOf`). */
  firstChoice(): string | undefined {
    return entry.firstChoiceOf(this.workspaces, this.me && this.#lastWorkspace?.(this.me.id));
  }

  /**
   * Shows the workspace `workspaceId`: cached sidebar data is shown immediately
   * when available and revalidated in the background (stale-while-revalidate);
   * what was shown of another is parked or cleared. Nothing is done when it is
   * the one entered or being entered, unless `again`.
   */
  async enter(workspaceId: string, again = false): Promise<void> {
    if (!again && this.asked === workspaceId && this.entered !== 'failed') return;
    const [turn, session] = [this.#entering + 1, this.#sessionGen];
    try {
      await this.#enter(workspaceId, again);
    } catch (error) {
      if (session !== this.#sessionGen || this.ends(error, true) || turn !== this.#entering) return;
      this.entered = 'failed';
      this.report('load', error);
    }
  }

  /** Entries begun: the latest is the one that counts. */
  #entering = 0;

  async #enter(workspaceId: string, again = false): Promise<void> {
    this.#entering += 1;
    const [turn, session] = [this.#entering, this.#sessionGen];
    this.#leave(workspaceId, true);
    const workspace = this.workspaces.find((one) => one.id === workspaceId);
    if (!workspace) {
      this.#caches.evictWorkspace(workspaceId);
      this.entered = 'missing';
      return;
    }
    const cached =
      !again && this.me ? this.#caches.workspaces.get(this.me.id, workspaceId) : undefined;
    if (cached && entry.isOpen(workspace)) {
      await this.#enterCached(session, turn, workspace, cached);
      return;
    }
    this.info = { ...workspace }; // Each entry its own: a way back is not "still there".
    this.entered = 'loading';
    this.#enteringWorkspaceId = workspaceId;
    const evictionGen = this.#visitEvictionGen;
    let read: entry.Entered;
    try {
      read = await entry.enterWorkspace(this.backend, workspace);
    } finally {
      if (turn === this.#entering) this.#enteringWorkspaceId = undefined;
    }
    if (session !== this.#sessionGen) return;
    if (read.status !== 'ready') {
      this.#caches.evictForEntered(this.workspaces, workspaceId, read, this.board);
    }
    if (turn !== this.#entering) return;
    [this.entered, this.info, this.closed] = [read.status, read.info, read.closed];
    this.workspaces = entry.withClosed(this.workspaces, read.closed);
    if (read.status === 'ready') {
      ({ users: this.users, containers: this.containers, categories: this.categories } = read);
      this.workspaceUsers = read.users;
      if (this.#visitEvictionGen === evictionGen) {
        this.#cacheableVisit = true;
        this.#rememberWorkspace();
      }
      this.#caches.pruneContainers(workspaceId, this.containers, this.board);
      this.#flushPendingResync(turn, workspaceId);
    }
  }

  async #enterCached(
    session: number,
    turn: number,
    workspace: WorkspaceInfo,
    cached: WorkspaceSnapshot,
  ): Promise<void> {
    this.#enteringWorkspaceId = workspace.id;
    const evictionGen = this.#visitEvictionGen;
    this.#cacheableVisit = true;
    try {
      await enterCachedWorkspace(
        this,
        this.#caches,
        {
          sidebarWrites: this.#sidebarWrites,
          isSession: (s) => s === this.#sessionGen,
          isCurrent: (t, visit) => t === this.#entering && this.info === visit,
          refreshSidebar: () => {
            this.#pendingResync = true;
          },
          rememberWorkspace: () => {
            if (this.#visitEvictionGen !== evictionGen) return;
            this.#cacheableVisit = true;
            this.#rememberWorkspace();
          },
          leaveWithoutCaching: (asked) => {
            this.#leave(asked, false);
          },
        },
        session,
        turn,
        workspace,
        cached,
      );
    } finally {
      if (turn === this.#entering) {
        this.#enteringWorkspaceId = undefined;
        this.#flushPendingResync(turn, workspace.id);
      }
    }
  }

  #flushPendingResync(turn: number, workspaceId: string): void {
    if (!this.#pendingResync) return;
    this.#pendingResync = false;
    if (turn === this.#entering && this.info?.id === workspaceId) void this.#refresh();
  }

  #rememberWorkspace(): void {
    rememberWorkspaceSnapshot(this.#caches, this, this.#cacheableVisit);
  }

  /** Lets go of the workspace shown, saving its sidebar snapshot when `cache` is true. */
  #leave(asked: string | undefined, cache = false): void {
    if (cache) {
      this.#rememberWorkspace();
      this.#caches.parkBoard(this);
    } else {
      this.#caches.dropBoard(this);
    }
    this.#cacheableVisit = false;
    this.#pendingResync = false;
    this.undo.clear();
    this.asked = asked;
    [this.info, this.closed] = [undefined, undefined];
    this.entered = 'none';
    [this.users, this.workspaceUsers, this.containers, this.categories] = [[], [], [], []];
  }

  /**
   * The session is over, or opens nothing (`sessionOver`). Nothing listens any
   * more and every navigation cache is wiped so nothing survives across sessions.
   */
  #over(status: 'signedOut' | 'leaving'): void {
    this.#sessionGen += 1;
    this.status = status;
    this.#leave(undefined, false);
    this.workspaces = [];
    this.close();
  }

  /** Stops listening and clears every cache. For a test, or when the reader signs out. */
  close(): void {
    this.#entering += 1;
    this.#refreshing += 1;
    this.#pendingResync = false;
    this.#enteringWorkspaceId = undefined;
    this.#stopWatching?.();
    this.#caches.dropBoard(this);
    this.#caches.clear();
  }

  #takeSettings(settings: AccountSettings): void {
    this.settings = settings;
    setRegionalSettings(settings);
    setDisplaySettings(settings);
  }

  #apply(change: Change): void {
    switch (change.kind) {
      case 'container': {
        // A board of another workspace than the one shown is not of its sidebar.
        if (change.container.workspaceId !== this.info?.id) break;
        // Two tellings of one board may arrive out of order: the older one is let go.
        const known = this.containers.find((board) => board.id === change.container.id);
        if (known && known.updateAt > change.container.updateAt) break;
        // A template is no board of the sidebar, as the server's list of boards says.
        const others = this.containers.filter((board) => board.id !== change.container.id);
        this.containers = change.container.isTemplate ? others : [...others, change.container];
        break;
      }
      case 'containerDeleted':
        this.#caches.evictContainer(change.containerId, this.board);
        this.containers = this.containers.filter((board) => board.id !== change.containerId);
        // A deleted board leaves every category (no message says so). Brought
        // back, it is filed again where it was, and the sidebar is told.
        this.categories = withoutContainer(this.categories, change.containerId);
        break;
      case 'categories':
        // Of this workspace, after pending writes: an earlier echo must not undo a move.
        if (this.#sidebarWrites.accepts(change.categories, this.info?.id)) {
          this.categories = change.categories;
        }
        break;
      case 'settings':
        this.#takeSettings(change.settings);
        break;
      case 'resync':
        if (this.info) {
          this.#cacheableVisit = false;
          this.#caches.workspaces.delete(this.info.id);
        }
        void this.#refresh();
        void this.#userNames.reread();
        break;
      case 'user':
        void this.#userNames.reread([change.userId]);
        break;
      default:
        break;
    }
  }

  /**
   * Reads the boards and the sidebar again after changes may have been
   * missed (`resync`): the boards merged with what is shown (`reread`). Of
   * two readings that overlap, the later one counts.
   */
  async #refresh(): Promise<void> {
    const workspace = this.info;
    if (this.status !== 'ready' || !workspace) return;
    if (this.#enteringWorkspaceId === workspace.id) {
      this.#pendingResync = true;
      return;
    }
    this.#refreshing += 1;
    const [session, reading] = [this.#sessionGen, this.#refreshing];
    await refreshWorkspaceSidebar(
      this,
      this.#caches,
      {
        sidebarWrites: this.#sidebarWrites,
        isSession: (s) => s === this.#sessionGen,
        isCurrent: (r, ws) => r === this.#refreshing && this.info === ws,
        refreshAgain: () => void this.#refresh(),
        rememberWorkspace: () => {
          this.#cacheableVisit = true;
          this.#rememberWorkspace();
        },
        leaveWithoutCaching: (asked) => {
          this.#leave(asked, false);
        },
      },
      session,
      reading,
      workspace,
    );
  }

  /**
   * Puts a failure where the shell shows it; an error that is not the server's is a bug,
   * and is logged too. A 401 is no failure to show (`ends`).
   */
  report(action: ProblemAction, error: unknown): void {
    if (this.ends(error, action === 'load')) return;
    if (isBackendError(error) && error.status === 403) {
      if (this.info) {
        this.#cacheableVisit = false;
        this.#caches.workspaces.delete(this.info.id);
      }
      if (this.board) this.#caches.boards.delete(this.board.boardId, this.board);
    }
    this.#problemCount += 1;
    this.problems = [
      ...this.problems,
      { id: this.#problemCount, action, error: isBackendError(error) ? error : undefined },
    ];
    if (error !== undefined && !isBackendError(error)) console.error(error);
  }

  /** Whether `error` ends the session or closes the workspace shown (a 401), as it then does. */
  ends(error: unknown, reading = false): boolean {
    // An organization began to take no password, or the own space the password alone: it closes.
    const closing = entry.closedBy(this.workspaces, this.info, error);
    if (closing) {
      if (reading && this.showing && closing.entered === 'password') return true;
      this.#caches.evictForClosing(this, error, closing);
      this.workspaces = closing.workspaces;
      if (!closing.closed) return true;
      this.#entering += 1; // What is still being read of it does not open it again.
      this.#leave(this.asked, false);
      [this.closed, this.entered] = [closing.closed, closing.entered];
      return true;
    }
    // The session is over (it went idle, or past its end): sign in again.
    const over = sessionOver(error);
    if (over) this.#over(over);
    return over !== undefined;
  }

  dismissProblem(id: number): void {
    this.problems = this.problems.filter((problem) => problem.id !== id);
  }

  readonly #userNames: UserNames;

  /**
   * Reads the people among `userIds` the workspace has no name for yet: the
   * members of a board, say, when the list of the workspace did not bring them.
   * Failing, the names stay unknown and nothing is told: nothing else
   * depends on them. A session that is over still signs out. (`UserNames`.)
   */
  async learnUsers(userIds: readonly string[], learning: Learning = {}): Promise<void> {
    await this.#userNames.learn(userIds, learning);
  }

  /**
   * People of the workspace whose name has `query` in it (ten at most). A failure
   * finds nobody; a session that is over signs out.
   */
  searchUsers(query: string): Promise<User[]> {
    return this.#userNames.search(query);
  }

  // ----------------------------------------------------------- settings

  /** Changes settings. They apply at once and go back if the server refuses them. */
  async changeSettings(patch: AccountSettingsPatch): Promise<void> {
    const before = this.settings;
    const optimistic: Record<string, unknown> = { ...before };
    for (const [name, value] of Object.entries(patch)) {
      optimistic[name] = value ?? DEFAULT_ACCOUNT_SETTINGS[name as keyof AccountSettings];
    }
    this.#takeSettings(optimistic as unknown as AccountSettings);
    try {
      this.#takeSettings(await this.backend.patchSettings(patch));
    } catch (error) {
      this.#takeSettings(before);
      this.report('save', error);
    }
  }

  // ------------------------------------------------------------- boards

  /**
   * Opens a board: resumes a cached store immediately (refreshing in the
   * background) or loads a new one. The board that was open is parked in cache
   * when reusable, else closed. The returned store is also `this.board`.
   */
  async openBoard(boardId: string): Promise<BoardStore> {
    // A board that went missing or failed is read anew: it may have been brought back.
    if (this.board?.boardId === boardId && this.board.status !== 'failed' && !this.board.missing) {
      return this.board;
    }
    this.#caches.parkBoard(this);
    const cached =
      this.me && this.info
        ? this.#caches.boards.take(this.me.id, this.info.id, boardId)
        : undefined;
    if (cached) {
      this.board = cached;
      await cached.resume();
      this.#caches.trackBoard(this, cached);
      return cached;
    }
    const store = new BoardStore(this, boardId);
    this.board = store;
    await store.load();
    this.#caches.trackBoard(this, store);
    return store;
  }

  /** Parks the open board in cache if reusable, else closes it. */
  closeBoard(): void {
    this.#caches.parkBoard(this);
  }

  // ------------------------------------------------------------- caches

  #invalidateVisit(): void {
    this.#visitEvictionGen += 1;
    this.#refreshing += 1;
    this.#cacheableVisit = false;
    this.board?.markEvicted();
  }

  evictWorkspace(workspaceId: string): void {
    if (this.info?.id === workspaceId || this.#enteringWorkspaceId === workspaceId) {
      this.#invalidateVisit();
    }
    this.#caches.evictWorkspace(workspaceId, this.board);
  }

  evictOrg(orgId: string): void {
    const entering = this.workspaces.find((one) => one.id === this.#enteringWorkspaceId);
    if (this.info?.orgId === orgId || entering?.orgId === orgId) this.#invalidateVisit();
    this.#caches.evictOrg(this.workspaces, orgId, this.board);
  }

  evictContainer(containerId: string): void {
    this.#caches.evictContainer(containerId, this.board);
  }

  pruneWorkspaces(): void {
    const current = this.info && this.workspaces.find((one) => one.id === this.info?.id);
    if (this.info && (!current || !entry.isOpen(current))) this.#invalidateVisit();
    this.#caches.pruneWorkspaces(this.workspaces, this.board);
  }

  rememberedWikiPage(wikiId: string): string | undefined {
    return this.me && this.info
      ? this.#caches.wikis.get(this.me.id, this.info.id, wikiId)
      : undefined;
  }

  rememberWikiPage(wikiId: string, pageId: string): void {
    if (this.me && this.info) this.#caches.wikis.set(this.me.id, this.info.id, wikiId, pageId);
  }

  forgetWikiPage(wikiId: string, pageId?: string): void {
    if (pageId === undefined) this.#caches.wikis.delete(wikiId);
    else this.#caches.wikis.deletePage(wikiId, pageId);
  }

  /**
   * Makes a board and its first view, a kanban grouped by the board's first
   * select property (`createBoardOf`).
   */
  createBoard(
    draft: BoardDraft,
    viewTitle = '',
    statusName = 'Status',
  ): Promise<Container | undefined> {
    return entry.createBoardOf(this, draft, viewTitle, statusName);
  }

  /**
   * The boards the reader may find by `query` (every word of the title) in each of their
   * workspaces the session opens, oldest first. A failure finds nothing there; a session over signs out.
   */
  searchBoards(query: string): Promise<Container[]> {
    return entry.searchBoardsOf(this, query);
  }

  /** The templates a board can be made from; none when they cannot be read (told). */
  templates(): Promise<Container[]> {
    return entry.templatesOf(this);
  }

  /** A board or a wiki as the backend just answered it: in the sidebar at once (`createWiki`). */
  receiveContainer(container: Container): void {
    this.#apply({ kind: 'container', container });
  }

  /** Copies a board, a wiki or a template into a board (`duplicateContainer`), in the sidebar at once. */
  duplicateContainer(containerId: string): Promise<Container | undefined> {
    return entry.duplicateContainerOf(this, containerId, false);
  }

  /** Makes a template of the workspace from a board. A template is no board of the sidebar. */
  createTemplateFromBoard(boardId: string): Promise<Container | undefined> {
    return entry.duplicateContainerOf(this, boardId, true);
  }

  /**
   * The welcome board of a new account, copied for them: called once, right
   * after the account is made (each call makes another copy). Undefined when
   * it could not be made; the reader starts with an empty workspace then.
   */
  onboard(): Promise<string | undefined> {
    return entry.onboardOf(this);
  }

  // ----------------------------------------------------------- archives

  /** The archives and the files on their way down or up, for the reader to watch. */
  readonly transfers = new Transfers(this);

  /** An archive of the board or wiki, for the shell to save. */
  exportContainer(containerId: string): Promise<DownloadedFile | undefined> {
    const board = this.containers.find((one) => one.id === containerId);
    return this.transfers.run('export', `board ${containerId}`, board?.title ?? '', (options) =>
      this.backend.exportContainer(containerId, options),
    );
  }

  /** An archive of every board and wiki of the workspace the reader may see. */
  exportWorkspace(): Promise<DownloadedFile | undefined> {
    const workspace = this.info;
    if (!workspace) return Promise.resolve(undefined);
    return this.transfers.run('export', `workspace ${workspace.id}`, workspace.title, (options) =>
      this.backend.exportWorkspace(workspace.id, options),
    );
  }

  /**
   * Imports the boards and wikis of an archive, then reads them and the sidebar
   * again: the server tells the new boards before their members exist, so
   * nobody hears of them. What it left out, if it went through.
   */
  async importArchive(file: File): Promise<Imported | undefined> {
    const workspace = this.info;
    if (!workspace) return undefined;
    const done = await this.transfers.run(
      'import',
      `import ${file.name} ${file.size} ${file.lastModified}`,
      file.name,
      (options) => this.backend.importArchive(workspace.id, file, options),
    );
    // Even a failed import may have made some boards (one after the other).
    void this.#refresh();
    return done;
  }

  /** Deletes a board. It can be brought back with undo. */
  async deleteContainer(containerId: string): Promise<void> {
    try {
      await this.undo.perform(
        async () => {
          await this.backend.deleteContainer(containerId);
          this.#apply({ kind: 'containerDeleted', containerId });
        },
        () => this.restoreContainer(containerId),
        'deleteContainer',
      );
    } catch (error) {
      this.report('save', error);
    }
  }

  /** Brings a deleted board or wiki back, the sidebar read first so it shows where it was filed. */
  async restoreContainer(containerId: string): Promise<void> {
    const container = await this.backend.restoreContainer(containerId);
    await this.#refresh();
    this.#apply({ kind: 'container', container });
  }

  // --------------------------------------------------------- categories

  readonly #sidebarWrites = new SidebarWrites(() => {
    void this.#refresh();
  });

  readonly #categoryOps = new WorkspaceCategories(this, this.#sidebarWrites, () => {
    void this.#refresh();
  });

  /** A new category goes first, as the backend puts it. */
  createCategory(name: string): Promise<void> {
    return this.#categoryOps.create(name);
  }

  renameCategory(categoryId: string, name: string): Promise<void> {
    return this.#categoryOps.rename(categoryId, name);
  }

  setCategoryCollapsed(categoryId: string, collapsed: boolean): Promise<void> {
    return this.#categoryOps.setCollapsed(categoryId, collapsed);
  }

  deleteCategory(categoryId: string): Promise<void> {
    return this.#categoryOps.remove(categoryId);
  }

  /** Sets the order of the categories; `categoryIds` names every one of them. */
  reorderCategories(categoryIds: readonly string[]): Promise<void> {
    return this.#categoryOps.reorder(categoryIds);
  }

  /** Files a board under a category, at `index` among the boards shown there. */
  moveContainer(containerId: string, categoryId: string, index: number): Promise<void> {
    return this.#categoryOps.moveContainer(containerId, categoryId, index);
  }

  /** Hides a board or a wiki in its category, out of the sidebar. Hiding can be undone. */
  hideContainer(containerId: string): Promise<void> {
    return this.#categoryOps.hideContainer(containerId);
  }

  /** Shows a hidden board in the sidebar again, in the category that hides it. */
  unhideContainer(containerId: string): Promise<void> {
    return this.#categoryOps.unhideContainer(containerId);
  }

  // --------------------------------------------------------------- undo

  async undoLast(): Promise<void> {
    try {
      await this.undo.undo();
    } catch (error) {
      this.report('undo', error);
    }
  }

  async redoLast(): Promise<void> {
    try {
      await this.undo.redo();
    } catch (error) {
      this.report('undo', error);
    }
  }
}
