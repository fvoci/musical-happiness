/**
 * The reader's workspace: who they are, the people around them, the boards
 * they can see, their sidebar and their settings. One per app.
 *
 * Screens read the fields and call the methods; the methods talk to the
 * backend. A write shows at once and is taken back if the backend refuses
 * it (see `write.ts`); what went wrong is put into `problems` for the shell
 * to tell the reader.
 */
import { setDisplaySettings } from '../display.svelte.ts';
import { DEFAULT_DISPLAY_SETTINGS } from '../logic/display.ts';
import { DEFAULT_LANGUAGE_SETTINGS } from '../logic/language.ts';
import { DEFAULT_REGIONAL_SETTINGS, resolveRegional, type Regional } from '../logic/regional.ts';
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
  type Team,
  type User,
} from './backend.ts';
import { BoardStore } from './board.svelte.ts';
import { hiddenBoardsOf, sidebarOf, type SidebarCategory } from './order.ts';
import { reread } from './write.ts';

/** An archive on its way down or up, for the reader to watch. */
export interface Transfer {
  id: number;
  kind: 'export' | 'import';
  /** What is moved: the board's title, the team's, or the archive's file name. */
  name: string;
  done: number;
  /** 0 while the length is not known. */
  total: number;
  finished: boolean;
}

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
}

/** How long a finished transfer stays in sight, in milliseconds. */
const TRANSFER_STAYS = 6000;

const DEFAULT_SETTINGS: AccountSettings = {
  ...DEFAULT_REGIONAL_SETTINGS,
  ...DEFAULT_DISPLAY_SETTINGS,
  ...DEFAULT_LANGUAGE_SETTINGS,
};

export class Workspace {
  readonly backend: Backend;
  readonly #locale: () => string;

  status = $state<LoadStatus>('idle');
  me = $state.raw<User | undefined>(undefined);
  team = $state.raw<Team | undefined>(undefined);
  users = $state.raw<User[]>([]);
  /** The team's list as the backend gives it without a search: on the server, its oldest ten (D2). */
  teamUsers = $state.raw<User[]>([]);
  boards = $state.raw<Board[]>([]);
  categories = $state.raw<Category[]>([]);
  settings = $state.raw<AccountSettings>(DEFAULT_SETTINGS);
  problems = $state.raw<Problem[]>([]);
  transfers = $state.raw<Transfer[]>([]);

  /** The board that is open, if one is. */
  board = $state.raw<BoardStore | undefined>(undefined);

  /** Undo and redo of what the reader did in this visit. */
  readonly undo = new UndoManager();
  canUndo = $state(false);
  canRedo = $state(false);

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
    this.undo.onStateDidChange = () => {
      this.canUndo = this.undo.canUndo;
      this.canRedo = this.undo.canRedo;
    };
  }

  get locale(): string {
    return this.#locale();
  }

  user(userId: string): User | undefined {
    return this.#usersById[userId];
  }

  /**
   * The name to show for a user, "Display name (@login)" or the login name
   * alone; undefined for someone the reader cannot see.
   */
  userName(userId: string): string | undefined {
    const user = this.#usersById[userId];
    return user ? userDisplayName(user) : undefined;
  }

  /** Reads everything the shell needs. Call once; call again to retry after a failure. */
  async load(): Promise<void> {
    this.status = 'loading';
    try {
      const [me, teams, settings] = await Promise.all([
        this.backend.me(),
        this.backend.teams(),
        this.backend.settings(),
      ]);
      const team = teams[0];
      if (!team) throw new Error('the reader belongs to no team');
      const [users, boards, categories] = await Promise.all([
        this.backend.users(team.id),
        this.backend.boards(team.id),
        this.backend.categories(team.id),
      ]);
      this.me = me;
      this.team = team;
      this.users = users;
      this.teamUsers = users;
      this.boards = boards;
      this.categories = categories;
      this.#takeSettings(settings);
      // The list of the team is short (ten): the open board's members it did
      // not bring are read again, as when the board was opened.
      if (this.board) void this.learnUsers(this.board.members.map((member) => member.userId));
      this.#stopWatching?.();
      this.#stopWatching = this.backend.watchSelf((change) => {
        this.#apply(change);
      });
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
        this.categories = change.categories;
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
    const team = this.team;
    if (this.status !== 'ready' || !team) return;
    this.#refreshing += 1;
    const reading = this.#refreshing;
    const before = this.boards.map((board) => board.id);
    const begun = this.#sidebarBegun;
    try {
      const [boards, categories] = await Promise.all([
        this.backend.boards(team.id),
        this.backend.categories(team.id),
      ]);
      if (reading !== this.#refreshing) return;
      this.boards = reread(this.boards, boards, before);
      // Not over a sidebar write under way, or one begun since this reading was.
      if (this.#sidebarWrites > 0) this.#sidebarStale = true;
      else if (begun !== this.#sidebarBegun) void this.#refresh();
      else this.categories = categories;
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
   * members of a board, say, when the list of the team did not bring them.
   * Failing, the names stay unknown and nothing is told: nothing else
   * depends on them. A session that is over still signs out.
   */
  async learnUsers(userIds: readonly string[]): Promise<void> {
    const team = this.team;
    const missing = userIds.filter(
      (id, at) => userIds.indexOf(id) === at && this.#usersById[id] === undefined,
    );
    if (!team || missing.length === 0) return;
    try {
      const found = await this.backend.usersById(team.id, missing);
      // Another read may have brought some of them meanwhile.
      this.users = [
        ...this.users,
        ...found.filter((user) => this.#usersById[user.id] === undefined),
      ];
    } catch (error) {
      if (isBackendError(error) && error.status === 401) this.report('load', error);
      else if (!isBackendError(error)) console.error(error);
    }
  }

  /**
   * People of the team whose name has `query` in it (ten at most). A failure
   * finds nobody; a session that is over signs out.
   */
  async searchUsers(query: string): Promise<User[]> {
    const team = this.team;
    if (!team) return [];
    try {
      return await this.backend.searchUsers(team.id, query);
    } catch (error) {
      if (isBackendError(error) && error.status === 401) this.report('load', error);
      else if (!isBackendError(error)) console.error(error);
      return [];
    }
  }

  // ----------------------------------------------------------- settings

  /**
   * Changes the reader's display name; empty is none. A refusal is thrown,
   * for the field to say what is wrong (`errors` at `/displayName`).
   */
  async changeDisplayName(displayName: string): Promise<void> {
    const changed = await this.backend.patchMe({ displayName });
    const replace = (users: User[]): User[] =>
      users.map((user) => (user.id === changed.id ? changed : user));
    this.me = changed;
    this.users = this.#usersById[changed.id] ? replace(this.users) : [...this.users, changed];
    this.teamUsers = replace(this.teamUsers);
  }

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
    if (!this.team) return undefined;
    try {
      const cardProperties =
        draft.cardProperties ?? createEmptyBoard(undefined, statusName).cardProperties;
      const groupBy = cardProperties.find((template) => template.type === 'select');
      const fields = createBoardView().fields;
      fields.viewType = 'board';
      fields.groupById = groupBy?.id;
      fields.visibleOptionIds = [...(groupBy?.options.map((option) => option.id) ?? []), ''];
      const { board } = await this.backend.createBoard(
        this.team.id,
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
   * The boards the reader may find by `query` (every word in the title).
   * A failure finds nothing; a session that is over signs out.
   */
  async searchBoards(query: string): Promise<Board[]> {
    const team = this.team;
    if (!team || query.trim() === '') return [];
    try {
      return await this.backend.searchBoards(team.id, query.trim());
    } catch (error) {
      if (isBackendError(error) && error.status === 401) this.report('load', error);
      else if (!isBackendError(error)) console.error(error);
      return [];
    }
  }

  /** The templates a board can be made from; none when they cannot be read (told). */
  async templates(): Promise<Board[]> {
    if (!this.team) return [];
    try {
      return await this.backend.templates(this.team.id);
    } catch (error) {
      this.report('load', error);
      return [];
    }
  }

  /** Makes a board from a template; it keeps the template's title, views and cards. */
  async createBoardFromTemplate(templateId: string): Promise<Board | undefined> {
    if (!this.team) return undefined;
    try {
      const board = await this.backend.duplicateBoard(templateId, false, this.team.id);
      this.#apply({ kind: 'board', board });
      return board;
    } catch (error) {
      this.report('save', error);
      return undefined;
    }
  }

  /** Makes a template of the team from a board. A template is no board of the sidebar. */
  async createTemplateFromBoard(boardId: string): Promise<Board | undefined> {
    if (!this.team) return undefined;
    try {
      return await this.backend.duplicateBoard(boardId, true, this.team.id);
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
    if (!this.team) return undefined;
    let boardId: string;
    try {
      boardId = await this.backend.onboard(this.team.id);
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

  #transferCount = 0;
  /** What stops each transfer under way, and what it moves (one at a time of each). */
  #running: { id: number; stop: AbortController; key: string }[] = [];

  /**
   * Runs a transfer that the reader can watch: its progress, its end. A
   * finished one stays a moment, then goes; a failed one is told as a
   * problem; one given up (dismissed while under way) is told nothing. The
   * same thing is not moved twice at once: asked again meanwhile, nothing
   * more is done.
   */
  async #transfer<T>(
    kind: Transfer['kind'],
    key: string,
    name: string,
    run: (options: {
      onProgress: (progress: { sentBytes: number; totalBytes: number }) => void;
      signal: AbortSignal;
    }) => Promise<T>,
  ): Promise<T | undefined> {
    if (this.#running.some((running) => running.key === key)) return undefined;
    this.#transferCount += 1;
    const id = this.#transferCount;
    const stop = new AbortController();
    this.#running = [...this.#running, { id, stop, key }];
    const change = (patch: Partial<Transfer>): void => {
      this.transfers = this.transfers.map((one) => (one.id === id ? { ...one, ...patch } : one));
    };
    this.transfers = [...this.transfers, { id, kind, name, done: 0, total: 0, finished: false }];
    try {
      const result = await run({
        onProgress: (progress) => {
          change({ done: progress.sentBytes, total: progress.totalBytes });
        },
        signal: stop.signal,
      });
      if (stop.signal.aborted) return undefined;
      change({ finished: true });
      setTimeout(() => {
        this.dismissTransfer(id);
      }, TRANSFER_STAYS);
      return result;
    } catch (error) {
      if (!stop.signal.aborted) {
        this.dismissTransfer(id);
        this.report(kind, error);
      }
      return undefined;
    } finally {
      this.#running = this.#running.filter((running) => running.id !== id);
    }
  }

  /** Takes a transfer out of sight; one under way is given up. */
  dismissTransfer(id: number): void {
    this.#running.find((running) => running.id === id)?.stop.abort();
    this.transfers = this.transfers.filter((one) => one.id !== id);
  }

  /** An archive of the board, for the shell to save. */
  exportBoard(boardId: string): Promise<DownloadedFile | undefined> {
    const board = this.boards.find((one) => one.id === boardId);
    return this.#transfer('export', `board ${boardId}`, board?.title ?? '', (options) =>
      this.backend.exportBoard(boardId, options),
    );
  }

  /** An archive of every board of the team the reader may see. */
  exportTeam(): Promise<DownloadedFile | undefined> {
    const team = this.team;
    if (!team) return Promise.resolve(undefined);
    return this.#transfer('export', `team ${team.id}`, team.title, (options) =>
      this.backend.exportTeam(team.id, options),
    );
  }

  /**
   * Imports the boards of an archive, then reads the boards and the sidebar
   * again: the server tells the new boards before their members exist, so
   * nobody hears of them. Whether it went through.
   */
  async importArchive(file: File): Promise<boolean> {
    const team = this.team;
    if (!team) return false;
    const done = await this.#transfer(
      'import',
      `import ${file.name} ${file.size} ${file.lastModified}`,
      file.name,
      async (options) => {
        await this.backend.importArchive(team.id, file, options);
        return true;
      },
    );
    // Even a failed import may have made some boards (one after the other).
    void this.#refresh();
    return done === true;
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

  /**
   * Brings a deleted board back. It shows where the reader had filed it
   * once the sidebar is told (a category message). A refusal is thrown.
   */
  async restoreBoard(boardId: string): Promise<void> {
    this.#apply({ kind: 'board', board: await this.backend.restoreBoard(boardId) });
  }

  // --------------------------------------------------------- categories

  /**
   * Shows a sidebar write at once. When it is refused the sidebar goes back
   * to what it was and is read again: what was taken back may be older than
   * what arrived from elsewhere meanwhile.
   */
  async #categoryWrite(optimistic: Category[], send: () => Promise<unknown>): Promise<void> {
    const before = this.categories;
    this.categories = optimistic;
    this.#sidebarWriting();
    try {
      await send();
    } catch (error) {
      this.#sidebarRefused(before);
      this.report('save', error);
    } finally {
      this.#sidebarWritten();
    }
  }

  /**
   * Sidebar writes under way. A reading of the sidebar meanwhile may be
   * older than what they show (a filing is two requests, and the first one's
   * message has the sidebar read between them): it is not shown, and the
   * sidebar is read again once the last write has ended.
   */
  #sidebarWrites = 0;
  /** Sidebar writes ever begun: a reading begun before one of them is older than it. */
  #sidebarBegun = 0;
  #sidebarStale = false;

  #sidebarWriting(): void {
    this.#sidebarWrites += 1;
    this.#sidebarBegun += 1;
  }

  #sidebarWritten(): void {
    this.#sidebarWrites -= 1;
    if (this.#sidebarWrites > 0 || !this.#sidebarStale) return;
    this.#sidebarStale = false;
    void this.#refresh();
  }

  #sidebarRefused(before: Category[]): void {
    this.categories = before;
    void this.#refresh();
  }

  /** A new category goes first, as the backend puts it. */
  async createCategory(name: string): Promise<void> {
    if (!this.team) return;
    this.#sidebarWriting();
    try {
      const category = await this.backend.createCategory(this.team.id, { name });
      if (this.categories.some((existing) => existing.id === category.id)) return;
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
      this.#sidebarWritten();
    }
  }

  async renameCategory(categoryId: string, name: string): Promise<void> {
    const team = this.team;
    if (!team) return;
    await this.#categoryWrite(
      this.categories.map((category) =>
        category.id === categoryId ? { ...category, name } : category,
      ),
      () => this.backend.patchCategory(team.id, categoryId, { name }),
    );
  }

  async setCategoryCollapsed(categoryId: string, collapsed: boolean): Promise<void> {
    const team = this.team;
    if (!team) return;
    await this.#categoryWrite(
      this.categories.map((category) =>
        category.id === categoryId ? { ...category, collapsed } : category,
      ),
      () => this.backend.patchCategory(team.id, categoryId, { collapsed }),
    );
  }

  async deleteCategory(categoryId: string): Promise<void> {
    const team = this.team;
    const gone = this.categories.find((category) => category.id === categoryId);
    if (!team || !gone) return;
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
      () => this.backend.deleteCategory(team.id, categoryId),
    );
  }

  /** Sets the order of the categories; `categoryIds` names every one of them. */
  async reorderCategories(categoryIds: readonly string[]): Promise<void> {
    const team = this.team;
    if (!team) return;
    await this.#categoryWrite(
      this.categories.map((category) => ({
        ...category,
        sortOrder: categoryIds.indexOf(category.id) * 10,
      })),
      () => this.backend.reorderCategories(team.id, [...categoryIds]),
    );
  }

  /** Files a board under a category, at `index` among the boards shown there. */
  async moveBoard(boardId: string, categoryId: string, index: number): Promise<void> {
    const team = this.team;
    if (!team) return;
    await this.#categoryWrite(
      this.categories.map((category) => {
        const boardIds = category.boardIds.filter((id) => id !== boardId);
        if (category.id === categoryId) {
          boardIds.splice(Math.max(0, Math.min(index, boardIds.length)), 0, boardId);
        }
        return { ...category, boardIds };
      }),
      () => this.backend.moveBoardToCategory(team.id, boardId, categoryId, index),
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
    const team = this.team;
    if (!team) return;
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
    this.#sidebarWriting();
    try {
      if (loose) {
        await this.backend.moveBoardToCategory(
          team.id,
          boardId,
          categoryId,
          category.boardIds.length,
        );
      }
      await this.backend.setBoardHidden(team.id, categoryId, boardId, hidden);
    } catch (error) {
      this.#sidebarRefused(before);
      throw error;
    } finally {
      this.#sidebarWritten();
    }
  }

  // --------------------------------------------------------------- undo

  async undoLast(): Promise<void> {
    if (!this.undo.canUndo) return;
    try {
      await this.undo.undo();
    } catch (error) {
      this.report('undo', error);
    }
  }

  async redoLast(): Promise<void> {
    if (!this.undo.canRedo) return;
    try {
      await this.undo.redo();
    } catch (error) {
      this.report('undo', error);
    }
  }
}
