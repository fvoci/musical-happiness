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
import { DEFAULT_REGIONAL_SETTINGS, resolveRegional, type Regional } from '../logic/regional.ts';
import { UndoManager } from '../logic/undo.ts';
import { SHOW_NICKNAME_FULL_NAME, userDisplayName } from '../logic/users.ts';
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
  type Team,
  type User,
} from './backend.ts';
import { BoardStore } from './board.svelte.ts';
import { hiddenBoardsOf, sidebarOf, type SidebarCategory } from './order.ts';
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

export type ProblemAction = 'load' | 'save' | 'undo' | 'upload' | 'conflict';

export interface WorkspaceOptions {
  backend: Backend;
  /** The language the app is shown in, a BCP 47 tag. Decides alphabetical order. */
  locale: () => string;
}

const DEFAULT_SETTINGS: AccountSettings = {
  ...DEFAULT_REGIONAL_SETTINGS,
  ...DEFAULT_DISPLAY_SETTINGS,
};

export class Workspace {
  readonly backend: Backend;
  readonly #locale: () => string;

  status = $state<LoadStatus>('idle');
  me = $state.raw<User | undefined>(undefined);
  team = $state.raw<Team | undefined>(undefined);
  users = $state.raw<User[]>([]);
  boards = $state.raw<Board[]>([]);
  categories = $state.raw<Category[]>([]);
  settings = $state.raw<AccountSettings>(DEFAULT_SETTINGS);
  problems = $state.raw<Problem[]>([]);

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

  /** How names are shown. The server will say; until its contract does, nickname first. */
  readonly nameFormat = SHOW_NICKNAME_FULL_NAME;

  user(userId: string): User | undefined {
    return this.#usersById[userId];
  }

  /** The name to show for a user; undefined for someone the reader cannot see. */
  userName(userId: string): string | undefined {
    const user = this.#usersById[userId];
    return user ? userDisplayName(user, this.nameFormat) : undefined;
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
        // A template is no board of the sidebar, as the server's list of boards says.
        const others = this.boards.filter((board) => board.id !== change.board.id);
        this.boards = change.board.isTemplate ? others : [...others, change.board];
        break;
      }
      case 'boardDeleted':
        this.boards = this.boards.filter((board) => board.id !== change.boardId);
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
    try {
      const [boards, categories] = await Promise.all([
        this.backend.boards(team.id),
        this.backend.categories(team.id),
      ]);
      if (reading !== this.#refreshing) return;
      this.boards = reread(this.boards, boards, before);
      this.categories = categories;
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
    if (this.board?.boardId === boardId) return this.board;
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
   * select property (a board without card properties is given a "Status"
   * one). `viewTitle` is the title of that view; the caller passes the
   * translated word. The two are made together.
   */
  async createBoard(draft: BoardDraft, viewTitle = ''): Promise<Board | undefined> {
    if (!this.team) return undefined;
    try {
      const cardProperties = draft.cardProperties ?? createEmptyBoard().cardProperties;
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

  /** Deletes a board. It can be brought back with undo. */
  async deleteBoard(boardId: string): Promise<void> {
    try {
      await this.undo.perform(
        async () => {
          await this.backend.deleteBoard(boardId);
          this.#apply({ kind: 'boardDeleted', boardId });
        },
        async () => {
          this.#apply({ kind: 'board', board: await this.backend.restoreBoard(boardId) });
        },
        'deleteBoard',
      );
    } catch (error) {
      this.report('save', error);
    }
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
    try {
      await send();
    } catch (error) {
      this.#sidebarRefused(before);
      this.report('save', error);
    }
  }

  #sidebarRefused(before: Category[]): void {
    this.categories = before;
    void this.#refresh();
  }

  /** A new category goes first, as the backend puts it. */
  async createCategory(name: string): Promise<void> {
    if (!this.team) return;
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
    this.categories = before.map((category) => {
      if (category.id !== categoryId) return category;
      const others = category.hiddenBoardIds.filter((id) => id !== boardId);
      return { ...category, hiddenBoardIds: hidden ? [...others, boardId] : others };
    });
    try {
      await this.backend.setBoardHidden(team.id, categoryId, boardId, hidden);
    } catch (error) {
      this.#sidebarRefused(before);
      throw error;
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
