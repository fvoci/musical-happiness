import { UserNames } from './user-names.ts';
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
import { createBoard as createEmptyBoard, createBoardView, type Board } from '../logic/wire.ts';
import { browserTimeZone, setRegionalSettings } from '../regional.ts';

import {
  isBackendError,
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
import { hiddenBoardsOf, sidebarOf, type SidebarCategory } from './order.ts';
import { SidebarWrites } from './sidebar-writes.ts';
import { Transfers } from './transfers.svelte.ts';
import { firstChoiceOf, readWorkspace, type EnterStatus } from './workspace-entry.ts';
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

export type ProblemAction = 'load' | 'save' | 'undo' | 'upload' | 'conflict' | 'export' | 'import';

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
  /** The workspace entered; undefined while none is, or the one asked for is missing. */
  info = $state.raw<WorkspaceInfo | undefined>(undefined);
  /** How the workspace last asked for (`asked`) stands. */
  entered = $state<EnterStatus>('none');
  /** The identifier of the workspace last asked for, whether or not it was found. */
  asked = $state<string | undefined>(undefined);
  users = $state.raw<User[]>([]);
  /** The workspace's list as the backend gives it without a search: on the server, its oldest ten (D2). */
  workspaceUsers = $state.raw<User[]>([]);
  boards = $state.raw<Board[]>([]);
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
    sidebarOf(this.categories, this.boards, this.locale),
  );

  /** The boards the reader hid from the sidebar, by title. */
  readonly hiddenBoards = $derived<Board[]>(
    hiddenBoardsOf(this.categories, this.boards, this.sidebar, this.locale),
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
      if (isBackendError(error) && error.status === 401) {
        this.#signedOut();
        return;
      }
      this.status = 'failed';
      this.report('load', error);
    }
  }

  /** The workspace to start in when the address names none (`firstChoiceOf`). */
  firstChoice(): string | undefined {
    return firstChoiceOf(this.workspaces, this.me && this.#lastWorkspace?.(this.me.id));
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
    this.backend.enter?.(workspace.id);
    const read = await readWorkspace(this.backend, workspace.id);
    if (turn !== this.#entering) return;
    if (!read) {
      this.info = undefined;
      this.entered = 'missing';
      return;
    }
    ({ users: this.users, boards: this.boards, categories: this.categories } = read);
    this.workspaceUsers = read.users;
    this.entered = 'ready';
  }

  /** Lets go of everything of the workspace shown: nothing of it stays for the next. */
  #leave(asked: string | undefined): void {
    this.board?.close();
    this.board = undefined;
    this.undo.clear();
    this.asked = asked;
    this.info = undefined;
    this.entered = 'none';
    this.users = [];
    this.workspaceUsers = [];
    this.boards = [];
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
      case 'board': {
        // A board of another workspace than the one shown is not of its sidebar.
        if (change.board.workspaceId !== this.info?.id) break;
        // Two tellings of one board may arrive out of order: the older one is let go.
        const known = this.boards.find((board) => board.id === change.board.id);
        if (known && known.updateAt > change.board.updateAt) break;
        // A template is no board of the sidebar, as the server's list of boards says.
        const others = this.boards.filter((board) => board.id !== change.board.id);
        this.boards = change.board.isTemplate ? others : [...others, change.board];
        break;
      }
      case 'boardDeleted':
        this.boards = this.boards.filter((board) => board.id !== change.boardId);
        // A deleted board leaves every category (no message says so). Brought
        // back, it is filed again where it was, and the sidebar is told.
        this.categories = this.categories.map((category) => ({
          ...category,
          boardIds: category.boardIds.filter((id) => id !== change.boardId),
          hiddenBoardIds: category.hiddenBoardIds.filter((id) => id !== change.boardId),
        }));
        break;
      case 'categories':
        // Of the workspace shown, or none: no sidebar of another replaces it.
        if (change.categories.every((one) => one.workspaceId === this.info?.id)) {
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
    const before = this.boards.map((board) => board.id);
    const begun = this.#sidebarWrites.begun;
    try {
      const [boards, categories] = await Promise.all([
        this.backend.boards(workspace.id),
        this.backend.categories(workspace.id),
      ]);
      // Superseded, or another workspace was entered meanwhile.
      if (reading !== this.#refreshing || this.info !== workspace) return;
      this.boards = reread(this.boards, boards, before);
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
   * is over, and the status turns to `signedOut`.
   */
  report(action: ProblemAction, error: unknown): void {
    // The session is over (it went idle, or past its end): sign in again.
    if (isBackendError(error) && error.status === 401) {
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

  /**
   * Reads the people among `userIds` the workspace has no name for yet: the
   * members of a board, say, when the list of the workspace did not bring them.
   * Failing, the names stay unknown and nothing is told: nothing else
   * depends on them. A session that is over still signs out.
   */
  readonly #userNames = new UserNames(this);

  async learnUsers(userIds: readonly string[]): Promise<void> {
    await this.#userNames.learn(userIds);
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
  ): Promise<Board | undefined> {
    if (!this.info) return undefined;
    try {
      const cardProperties =
        draft.cardProperties ?? createEmptyBoard(undefined, statusName).cardProperties;
      const groupBy = cardProperties.find((template) => template.type === 'select');
      const fields = createBoardView().fields;
      fields.viewType = 'board';
      fields.groupById = groupBy?.id;
      fields.visibleOptionIds = [...(groupBy?.options.map((option) => option.id) ?? []), ''];
      const { board } = await this.backend.createBoard(
        this.info.id,
        { ...draft, cardProperties },
        { title: viewTitle, fields },
      );
      this.#apply({ kind: 'board', board });
      return board;
    } catch (error) {
      this.report('save', error);
      return undefined;
    }
  }

  /**
   * The boards the reader may find by `query` (every word of the title) in each of their
   * workspaces, oldest first. A failure finds nothing there; a session over signs out.
   */
  async searchBoards(query: string): Promise<Board[]> {
    if (!this.info || query.trim() === '') return [];
    const found = this.workspaces.map((workspace) =>
      this.backend.searchBoards(workspace.id, query.trim()).catch((error: unknown) => {
        if (isBackendError(error) && error.status === 401) this.report('load', error);
        else if (!isBackendError(error)) console.error(error);
        return [];
      }),
    );
    return (await Promise.all(found)).flat().sort((one, other) => one.createAt - other.createAt);
  }

  /** The templates a board can be made from; none when they cannot be read (told). */
  async templates(): Promise<Board[]> {
    if (!this.info) return [];
    try {
      return await this.backend.templates(this.info.id);
    } catch (error) {
      this.report('load', error);
      return [];
    }
  }

  /** Makes a board from a template; it keeps the template's title, views and cards. */
  async createBoardFromTemplate(templateId: string): Promise<Board | undefined> {
    if (!this.info) return undefined;
    try {
      const board = await this.backend.duplicateBoard(templateId, false, this.info.id);
      this.#apply({ kind: 'board', board });
      return board;
    } catch (error) {
      this.report('save', error);
      return undefined;
    }
  }

  /** Makes a template of the workspace from a board. A template is no board of the sidebar. */
  async createTemplateFromBoard(boardId: string): Promise<Board | undefined> {
    if (!this.info) return undefined;
    try {
      return await this.backend.duplicateBoard(boardId, true, this.info.id);
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
      this.#apply({ kind: 'board', board: await this.backend.board(boardId) });
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

  /** An archive of the board, for the shell to save. */
  exportBoard(boardId: string): Promise<DownloadedFile | undefined> {
    const board = this.boards.find((one) => one.id === boardId);
    return this.transfers.run('export', `board ${boardId}`, board?.title ?? '', (options) =>
      this.backend.exportBoard(boardId, options),
    );
  }

  /** An archive of every board of the workspace the reader may see. */
  exportWorkspace(): Promise<DownloadedFile | undefined> {
    const workspace = this.info;
    if (!workspace) return Promise.resolve(undefined);
    return this.transfers.run('export', `workspace ${workspace.id}`, workspace.title, (options) =>
      this.backend.exportWorkspace(workspace.id, options),
    );
  }

  /**
   * Imports the boards of an archive, then reads the boards and the sidebar
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
  async deleteBoard(boardId: string): Promise<void> {
    try {
      await this.undo.perform(
        async () => {
          await this.backend.deleteBoard(boardId);
          this.#apply({ kind: 'boardDeleted', boardId });
        },
        () => this.restoreBoard(boardId),
        'deleteBoard',
      );
    } catch (error) {
      this.report('save', error);
    }
  }

  /** Brings a deleted board back, the sidebar read first so it shows where it was filed. */
  async restoreBoard(boardId: string): Promise<void> {
    const board = await this.backend.restoreBoard(boardId);
    await this.#refresh();
    this.#apply({ kind: 'board', board });
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
                boardIds: [...category.boardIds, ...gone.boardIds],
                hiddenBoardIds: [...category.hiddenBoardIds, ...gone.hiddenBoardIds],
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
  async moveBoard(boardId: string, categoryId: string, index: number): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    await this.#categoryWrite(
      this.categories.map((category) => {
        const boardIds = category.boardIds.filter((id) => id !== boardId);
        if (category.id === categoryId) {
          boardIds.splice(Math.max(0, Math.min(index, boardIds.length)), 0, boardId);
        }
        return { ...category, boardIds };
      }),
      () => this.backend.moveBoardToCategory(workspace.id, boardId, categoryId, index),
    );
  }

  /**
   * Takes a board out of the sidebar, or puts it back: it stays in its
   * category, hidden. Hiding can be undone.
   */
  async hideBoard(boardId: string): Promise<void> {
    const entry = this.sidebar.find((shown) => shown.boards.some((board) => board.id === boardId));
    if (!entry) return;
    const categoryId = entry.category.id;
    try {
      await this.undo.perform(
        () => this.#setBoardHidden(categoryId, boardId, true),
        () => this.#setBoardHidden(categoryId, boardId, false),
        'hideBoard',
      );
    } catch (error) {
      this.report('save', error);
    }
  }

  /** Shows a hidden board in the sidebar again, in the category that hides it. */
  async unhideBoard(boardId: string): Promise<void> {
    const category = this.categories.find((one) => one.hiddenBoardIds.includes(boardId));
    if (!category) return;
    try {
      await this.#setBoardHidden(category.id, boardId, false);
    } catch (error) {
      this.report('save', error);
    }
  }

  /**
   * Shows the change at once; when the backend refuses it, takes it back as
   * any sidebar write (`#sidebarRefused`) and rejects.
   */
  async #setBoardHidden(categoryId: string, boardId: string, hidden: boolean): Promise<void> {
    const workspace = this.info;
    if (!workspace) return;
    const before = this.categories;
    const category = before.find((one) => one.id === categoryId);
    // A board no category files (one brought back from the trash) is shown in
    // the default category but cannot be hidden there until it is filed there.
    const loose = hidden && category !== undefined && !category.boardIds.includes(boardId);
    this.categories = before.map((one) => {
      if (one.id !== categoryId) return one;
      const others = one.hiddenBoardIds.filter((id) => id !== boardId);
      return {
        ...one,
        boardIds: loose ? [...one.boardIds, boardId] : one.boardIds,
        hiddenBoardIds: hidden ? [...others, boardId] : others,
      };
    });
    this.#sidebarWrites.begin();
    try {
      if (loose) {
        await this.backend.moveBoardToCategory(
          workspace.id,
          boardId,
          categoryId,
          category.boardIds.length,
        );
      }
      await this.backend.setBoardHidden(workspace.id, categoryId, boardId, hidden);
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
