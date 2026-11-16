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
import { userDisplayName } from '../logic/users.ts';
import { createBoard as createEmptyBoard, type Container } from '../logic/wire.ts';
import { browserTimeZone, setRegionalSettings } from '../regional.ts';

import {
  isBackendError,
  isSignedOut,
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
import * as entry from './workspace-entry.ts';
import { reread } from './write.ts';

/** `signedOut`: the backend has accounts and nobody is signed in (401). */
export type LoadStatus = 'idle' | 'loading' | 'ready' | 'failed' | 'signedOut';

/** Something a write or a read failed with, for the reader to be told. */
export interface Problem {
  id: number;
  /** What was being done, as a key the shell has a text for. */
  action: ProblemAction;
  error: BackendError | undefined;
}

export type ProblemAction = 'load' | 'save' | 'undo' | 'upload' | 'export' | 'import';

export interface WorkspaceOptions {
  backend: Backend;
  /** The language the app is shown in, a BCP 47 tag. Decides alphabetical order. */
  locale: () => string;
  /**
   * The workspace this user was last in on this device, if the app keeps
   * one: entered when `load` is not told which.
   */
  lastWorkspace?: (userId: string) => string | undefined;
}

const DEFAULT_SETTINGS: AccountSettings = DEFAULT_ACCOUNT_SETTINGS;

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
  /** The workspace asked for, when it is theirs and closed to this session (`entered` is `sso`). */
  closed = $state.raw<WorkspaceInfo | undefined>(undefined);
  /** How the workspace last asked for (`asked`) stands. */
  entered = $state<entry.EnterStatus>('none');
  /** The identifier of the workspace last asked for, whether or not it was found. */
  asked = $state<string | undefined>(undefined);
  users = $state.raw<User[]>([]);
  /** The workspace's list as the backend gives it without a search: on the server, its oldest ten (D2). */
  workspaceUsers = $state.raw<User[]>([]);
  containers = $state.raw<Container[]>([]);
  categories = $state.raw<Category[]>([]);
  settings = $state.raw<AccountSettings>(DEFAULT_SETTINGS);
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

  constructor(options: WorkspaceOptions) {
    this.backend = options.backend;
    this.#locale = options.locale;
    this.#lastWorkspace = options.lastWorkspace;
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

  /** The name to show for a user ("Display name (@login)"); undefined for someone unknown. */
  userName(userId: string): string | undefined {
    const user = this.#usersById[userId];
    return user ? userDisplayName(user) : undefined;
  }

  /**
   * Reads everything the shell needs: the reader, their workspaces and settings, then the
   * workspace `workspaceId` (`enter`), or without one `firstChoice`. Call again to retry.
   */
  async load(workspaceId?: string): Promise<void> {
    this.status = 'loading';
    try {
      const [me, workspaces, settings] = await Promise.all([
        this.backend.me(),
        this.backend.workspaces(),
        this.backend.settings(),
      ]);
      this.me = me;
      this.workspaces = workspaces;
      this.#takeSettings(settings);
      this.#stopWatching?.();
      this.#stopWatching = this.backend.watchSelf((change) => {
        this.#apply(change);
      });
      const chosen = workspaceId ?? this.firstChoice();
      if (chosen === undefined) this.#leave(undefined);
      else await this.#enter(chosen);
      this.status = 'ready';
    } catch (error) {
      if (isSignedOut(error)) {
        this.#signedOut();
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
   * Shows the workspace `workspaceId`: its people, boards and sidebar are
   * read, and what was shown of another (the open board, undo) is let go.
   * Nothing is done when it is the one entered or being entered. A failure
   * is told; the status says it (`entered`).
   */
  async enter(workspaceId: string): Promise<void> {
    if (this.asked === workspaceId && this.entered !== 'failed') return;
    const turn = this.#entering + 1;
    try {
      await this.#enter(workspaceId);
    } catch (error) {
      if (turn !== this.#entering) return;
      this.entered = 'failed';
      this.report('load', error);
    }
  }

  /** Entries begun: the latest is the one that counts. */
  #entering = 0;

  async #enter(workspaceId: string): Promise<void> {
    this.#entering += 1;
    const turn = this.#entering;
    this.#leave(workspaceId);
    const workspace = this.workspaces.find((one) => one.id === workspaceId);
    if (!workspace) {
      this.entered = 'missing';
      return;
    }
    this.info = { ...workspace }; // Each entry its own: a way back is not "still there".
    this.entered = 'loading';
    const read = await entry.enterWorkspace(this.backend, workspace);
    if (turn !== this.#entering) return;
    [this.entered, this.info, this.closed] = [read.status, read.info, read.closed];
    if (read.status === 'ready') {
      ({ users: this.users, containers: this.containers, categories: this.categories } = read);
      this.workspaceUsers = read.users;
    }
  }

  /** Lets go of everything of the workspace shown: nothing of it stays for the next. */
  #leave(asked: string | undefined): void {
    this.board?.close();
    this.board = undefined;
    this.undo.clear();
    this.asked = asked;
    [this.info, this.closed] = [undefined, undefined];
    this.entered = 'none';
    this.users = [];
    this.workspaceUsers = [];
    this.containers = [];
    this.categories = [];
  }

  /**
   * The session is over. Nothing listens any more (a connection without a
   * session hears nothing, and would only be opened again and again):
   * signing in reads afresh, or starts the page again.
   */
  #signedOut(): void {
    this.status = 'signedOut';
    this.close();
  }

  /** Stops listening. For a test, or when the reader signs out. */
  close(): void {
    this.#stopWatching?.();
    this.board?.close();
    this.board = undefined;
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
        void this.#refresh();
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
    this.#refreshing += 1;
    const reading = this.#refreshing;
    const before = this.containers.map((board) => board.id);
    const begun = this.#sidebarWrites.begun;
    try {
      const [boards, categories] = await Promise.all([
        entry.modulesOf(this.backend, workspace.id),
        this.backend.categories(workspace.id),
      ]);
      // Superseded, or another workspace was entered meanwhile.
      if (reading !== this.#refreshing || this.info !== workspace) return;
      this.containers = reread(this.containers, boards, before);
      // Not over a sidebar write under way, or one begun since this reading was.
      const verdict = this.#sidebarWrites.reading(begun);
      if (verdict === 'show') this.categories = categories;
      else if (verdict === 'again') void this.#refresh();
    } catch (error) {
      // A reading in the background: only a session that is over is told
      // (as signing out). The next message, or the next load, brings the rest.
      if (reading === this.#refreshing && isBackendError(error) && error.status === 401) {
        this.report('load', error);
      }
    }
  }

  /**
   * Puts a failure where the shell shows it. Anything that is not a backend
   * error is a bug and is logged. A 401 is no failure to show: the session
   * is over (`signedOut`), or the workspace shown is closed to it (`sso`).
   */
  report(action: ProblemAction, error: unknown): void {
    // An organization began to take no password: what is shown of it closes, the session goes on.
    const closing = entry.closedBySso(this.workspaces, this.info, error);
    if (closing) {
      this.workspaces = closing.workspaces;
      if (!closing.closed) return;
      this.#entering += 1; // What is still being read of it does not open it again.
      this.#leave(this.asked);
      [this.closed, this.entered] = [closing.closed, 'sso'];
      return;
    }
    // The session is over (it went idle, or past its end): sign in again.
    if (isSignedOut(error)) {
      this.#signedOut();
      return;
    }
    this.#problemCount += 1;
    this.problems = [
      ...this.problems,
      { id: this.#problemCount, action, error: isBackendError(error) ? error : undefined },
    ];
    if (!isBackendError(error)) console.error(error);
  }

  dismissProblem(id: number): void {
    this.problems = this.problems.filter((problem) => problem.id !== id);
  }

  readonly #userNames = new UserNames(this);

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
  async searchUsers(query: string): Promise<User[]> {
    const workspace = this.info;
    if (!workspace) return [];
    try {
      return await this.backend.searchUsers(workspace.id, query);
    } catch (error) {
      if (isBackendError(error) && error.status === 401) this.report('load', error);
      else if (!isBackendError(error)) console.error(error);
      return [];
    }
  }

  // ----------------------------------------------------------- settings

  /** Changes settings. They apply at once and go back if the server refuses them. */
  async changeSettings(patch: AccountSettingsPatch): Promise<void> {
    const before = this.settings;
    const optimistic: Record<string, unknown> = { ...before };
    for (const [name, value] of Object.entries(patch)) {
      optimistic[name] = value ?? DEFAULT_SETTINGS[name as keyof AccountSettings];
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
   * Opens a board: reads it and starts following its changes. The board
   * that was open is closed. The returned store is also `this.board`.
   */
  async openBoard(boardId: string): Promise<BoardStore> {
    // A board that went missing is read anew: it may have been brought back.
    if (this.board?.boardId === boardId && !this.board.missing) return this.board;
    this.board?.close();
    const store = new BoardStore(this, boardId);
    this.board = store;
    await store.load();
    return store;
  }

  closeBoard(): void {
    this.board?.close();
    this.board = undefined;
  }

  /**
   * Makes a board and its first view, a kanban grouped by the board's first
   * select property (a board without card properties is given one named
   * `statusName`). `viewTitle` is the title of that view. The caller passes
   * both in the reader's language. The two are made together.
   */
  async createBoard(
    draft: BoardDraft,
    viewTitle = '',
    statusName = 'Status',
  ): Promise<Container | undefined> {
    if (!this.info) return undefined;
    try {
      const cardProperties =
        draft.cardProperties ?? createEmptyBoard(undefined, statusName).cardProperties;
      const { board } = await this.backend.createBoard(
        this.info.id,
        { ...draft, cardProperties },
        entry.firstKanban(cardProperties, viewTitle),
      );
      this.#apply({ kind: 'container', container: board });
      return board;
    } catch (error) {
      this.report('save', error);
      return undefined;
    }
  }

  /**
   * The boards the reader may find by `query` (every word of the title) in each of their
   * workspaces the session opens, oldest first. A failure finds nothing there; a session over signs out.
   */
  async searchBoards(query: string): Promise<Container[]> {
    if (!this.info || query.trim() === '') return [];
    const found = this.workspaces.filter(entry.isOpen).map((workspace) =>
      this.backend.searchBoards(workspace.id, query.trim()).catch((error: unknown) => {
        if (isBackendError(error) && error.status === 401) this.report('load', error);
        else if (!isBackendError(error)) console.error(error);
        return [];
      }),
    );
    return (await Promise.all(found)).flat().sort((one, other) => one.createAt - other.createAt);
  }

  /** The templates a board can be made from; none when they cannot be read (told). */
  templates(): Promise<Container[]> {
    return entry.templatesOf(this);
  }

  /** A board or a wiki as the backend just answered it: in the sidebar at once (`createWiki`). */
  receiveContainer(board: Container): void {
    this.#apply({ kind: 'container', container: board });
  }

  /** Copies a board, a wiki or a template into a board (`duplicateContainer`), in the sidebar at once. */
  async duplicateContainer(containerId: string): Promise<Container | undefined> {
    if (!this.info) return undefined;
    try {
      const board = await this.backend.duplicateContainer(containerId, false, this.info.id);
      this.#apply({ kind: 'container', container: board });
      return board;
    } catch (error) {
      this.report('save', error);
      return undefined;
    }
  }

  /** Makes a template of the workspace from a board. A template is no board of the sidebar. */
  async createTemplateFromBoard(boardId: string): Promise<Container | undefined> {
    if (!this.info) return undefined;
    try {
      return await this.backend.duplicateContainer(boardId, true, this.info.id);
    } catch (error) {
      this.report('save', error);
      return undefined;
    }
  }

  /**
   * The welcome board of a new account, copied for them: called once, right
   * after the account is made (each call makes another copy). Undefined when
   * it could not be made; the reader starts with an empty workspace then.
   */
  async onboard(): Promise<string | undefined> {
    if (!this.info) return undefined;
    let boardId: string;
    try {
      boardId = await this.backend.onboard(this.info.id);
    } catch (error) {
      this.report('load', error);
      return undefined;
    }
    try {
      this.#apply({ kind: 'container', container: await this.backend.container(boardId) });
    } catch {
      // It is made; the server's message, or the next reading, shows it.
    }
    return boardId;
  }

  // ----------------------------------------------------------- archives

  /** The archives on their way down or up, for the reader to watch. */
  readonly transfers = new Transfers((kind, error) => {
    this.report(kind, error);
  });

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

  /** Brings a deleted board back, the sidebar read first so it shows where it was filed. */
  async restoreContainer(containerId: string): Promise<void> {
    const board = await this.backend.restoreContainer(containerId);
    await this.#refresh();
    this.#apply({ kind: 'container', container: board });
  }

  // --------------------------------------------------------- categories

  /**
   * Shows a sidebar write at once. When it is refused the sidebar goes back
   * to what it was and is read again: what was taken back may be older than
   * what arrived from elsewhere meanwhile.
   */
  async #categoryWrite(optimistic: Category[], send: () => Promise<unknown>): Promise<void> {
    const [before, workspace] = [this.categories, this.info];
    this.categories = optimistic;
    this.#sidebarWrites.begin();
    try {
      await send();
    } catch (error) {
      this.#sidebarRefused(before, workspace);
      this.report('save', error);
    } finally {
      this.#sidebarWrites.end();
    }
  }

  readonly #sidebarWrites = new SidebarWrites(() => {
    void this.#refresh();
  });

  #sidebarRefused(before: Category[], workspace: WorkspaceInfo | undefined): void {
    if (this.info !== workspace) return;
    this.categories = before;
    void this.#refresh();
  }

  /** A new category goes first, as the backend puts it. */
  async createCategory(name: string): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    this.#sidebarWrites.begin();
    try {
      const category = await this.backend.createCategory(workspace.id, { name });
      // Shown already, or of a workspace left meanwhile: nothing to add.
      if (this.categories.some((existing) => existing.id === category.id)) return;
      if (this.info !== workspace) return;
      // The others were moved down to make room; until they are read again,
      // the new one is put before them.
      const first = Math.min(...this.categories.map((existing) => existing.sortOrder));
      this.categories = [
        ...this.categories,
        { ...category, sortOrder: Math.min(category.sortOrder, first - 1) },
      ];
    } catch (error) {
      this.report('save', error);
    } finally {
      this.#sidebarWrites.end();
    }
  }

  async renameCategory(categoryId: string, name: string): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    await this.#categoryWrite(
      this.categories.map((category) =>
        category.id === categoryId ? { ...category, name } : category,
      ),
      () => this.backend.patchCategory(workspace.id, categoryId, { name }),
    );
  }

  async setCategoryCollapsed(categoryId: string, collapsed: boolean): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    await this.#categoryWrite(
      this.categories.map((category) =>
        category.id === categoryId ? { ...category, collapsed } : category,
      ),
      () => this.backend.patchCategory(workspace.id, categoryId, { collapsed }),
    );
  }

  async deleteCategory(categoryId: string): Promise<void> {
    const workspace = this.info;
    const gone = this.categories.find((category) => category.id === categoryId);
    if (!workspace || !gone) return;
    await this.#categoryWrite(
      this.categories
        .filter((category) => category.id !== categoryId)
        .map((category) =>
          // Its boards go to the default category, those it hid still hidden.
          category.type === 'system'
            ? {
                ...category,
                containerIds: [...category.containerIds, ...gone.containerIds],
                hiddenContainerIds: [...category.hiddenContainerIds, ...gone.hiddenContainerIds],
              }
            : category,
        ),
      () => this.backend.deleteCategory(workspace.id, categoryId),
    );
  }

  /** Sets the order of the categories; `categoryIds` names every one of them. */
  async reorderCategories(categoryIds: readonly string[]): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    await this.#categoryWrite(
      this.categories.map((category) => ({
        ...category,
        sortOrder: categoryIds.indexOf(category.id) * 10,
      })),
      () => this.backend.reorderCategories(workspace.id, [...categoryIds]),
    );
  }

  /** Files a board under a category, at `index` among the boards shown there. */
  async moveContainer(containerId: string, categoryId: string, index: number): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    await this.#categoryWrite(
      this.categories.map((category) => {
        const containerIds = category.containerIds.filter((id) => id !== containerId);
        if (category.id === categoryId) {
          containerIds.splice(Math.max(0, Math.min(index, containerIds.length)), 0, containerId);
        }
        return { ...category, containerIds };
      }),
      () => this.backend.moveContainerToCategory(workspace.id, containerId, categoryId, index),
    );
  }

  /** Hides a board or a wiki in its category, out of the sidebar. Hiding can be undone. */
  async hideContainer(containerId: string): Promise<void> {
    const entry = this.sidebar.find((shown) =>
      shown.containers.some((board) => board.id === containerId),
    );
    if (!entry) return;
    const categoryId = entry.category.id;
    try {
      await this.undo.perform(
        () => this.#setContainerHidden(categoryId, containerId, true),
        () => this.#setContainerHidden(categoryId, containerId, false),
        'hideContainer',
      );
    } catch (error) {
      this.report('save', error);
    }
  }

  /** Shows a hidden board in the sidebar again, in the category that hides it. */
  async unhideContainer(containerId: string): Promise<void> {
    const category = this.categories.find((one) => one.hiddenContainerIds.includes(containerId));
    if (!category) return;
    try {
      await this.#setContainerHidden(category.id, containerId, false);
    } catch (error) {
      this.report('save', error);
    }
  }

  /**
   * Shows the change at once; when the backend refuses it, takes it back as
   * any sidebar write (`#sidebarRefused`) and rejects.
   */
  async #setContainerHidden(
    categoryId: string,
    containerId: string,
    hidden: boolean,
  ): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    const before = this.categories;
    const category = before.find((one) => one.id === categoryId);
    // A board no category files (one brought back from the trash) is shown in
    // the default category but cannot be hidden there until it is filed there.
    const loose = hidden && category !== undefined && !category.containerIds.includes(containerId);
    this.categories = before.map((one) => {
      if (one.id !== categoryId) return one;
      const others = one.hiddenContainerIds.filter((id) => id !== containerId);
      return {
        ...one,
        containerIds: loose ? [...one.containerIds, containerId] : one.containerIds,
        hiddenContainerIds: hidden ? [...others, containerId] : others,
      };
    });
    this.#sidebarWrites.begin();
    try {
      if (loose) {
        const filed = category.containerIds.length;
        await this.backend.moveContainerToCategory(workspace.id, containerId, categoryId, filed);
      }
      await this.backend.setContainerHidden(workspace.id, categoryId, containerId, hidden);
    } catch (error) {
      this.#sidebarRefused(before, workspace);
      throw error;
    } finally {
      this.#sidebarWrites.end();
    }
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
