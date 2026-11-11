/**
 * A list of what is in the trash, and bringing one back: the boards of the
 * team the reader was an admin of (`boardTrash`), or the cards of a board
 * (`cardTrash`, for its editors). Nothing is deleted for good here: the
 * server empties its trash after a while by itself.
 *
 * The list is read in pages, the last deleted first. Bringing one back takes
 * it out of the list, which moves the pages after it; so "more" reads the
 * whole of the list so far again, one page longer, instead of the page after.
 */
import { isBackendError, type DeletedItem, type Paging } from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import type { Workspace } from './workspace.svelte.ts';

/** How many a reading adds. */
export const TRASH_PAGE = 50;

export type TrashStatus = 'idle' | 'loading' | 'ready' | 'failed';

export class Trash {
  items = $state.raw<DeletedItem[]>([]);
  status = $state<TrashStatus>('idle');
  /** The last reading was full: there may be more. */
  hasMore = $state(false);
  /** Those on their way back. */
  restoring = $state.raw<string[]>([]);
  /** Those brought back: a reading begun before may still have them. */
  readonly #restored: string[] = [];

  readonly #workspace: Workspace;
  readonly #list: (paging: Paging) => Promise<DeletedItem[]>;
  readonly #restore: (id: string) => Promise<void>;
  /** How many the list asks for. */
  #length = TRASH_PAGE;
  /** Readings begun: the latest is the one that counts. */
  #readings = 0;

  constructor(
    workspace: Workspace,
    list: (paging: Paging) => Promise<DeletedItem[]>,
    restore: (id: string) => Promise<void>,
  ) {
    this.#workspace = workspace;
    this.#list = list;
    this.#restore = restore;
  }

  /** Reads the list from its start. */
  async read(): Promise<void> {
    this.#length = TRASH_PAGE;
    this.items = [];
    await this.#read();
  }

  /** Reads again what was asked for last, after a failure: the pages shown stay asked for. */
  async retry(): Promise<void> {
    if (this.status === 'loading') return;
    await this.#read();
  }

  /** Reads one page more. */
  async more(): Promise<void> {
    if (this.status === 'loading' || !this.hasMore) return;
    this.#length += TRASH_PAGE;
    await this.#read();
  }

  async #read(): Promise<void> {
    this.#readings += 1;
    const reading = this.#readings;
    const length = this.#length;
    this.status = 'loading';
    try {
      const items = await this.#list({ page: 0, perPage: length });
      if (reading !== this.#readings) return;
      // One on its way back, or back, is not shown again by a reading that began before.
      this.items = items.filter(
        (item) => !this.restoring.includes(item.id) && !this.#restored.includes(item.id),
      );
      this.hasMore = items.length === length;
      this.status = 'ready';
      const deleters = items.map((item) => item.deletedBy).filter((userId) => userId !== '');
      void this.#workspace.learnUsers(deleters);
    } catch (error) {
      if (reading !== this.#readings) return;
      // The list says it failed; a session that is over is the page's to tell.
      this.status = 'failed';
      if (isBackendError(error) && error.status === 401) this.#workspace.report('load', error);
    }
  }

  /**
   * Brings one back. Answers whether it is back; when it is not, the reader
   * is told why, and one the trash no longer has (404) leaves the list.
   */
  async restore(id: string): Promise<boolean> {
    if (this.restoring.includes(id)) return false;
    this.restoring = [...this.restoring, id];
    try {
      await this.#restore(id);
      this.#restored.push(id);
      this.items = this.items.filter((item) => item.id !== id);
      return true;
    } catch (error) {
      if (isBackendError(error) && error.status === 404) {
        this.items = this.items.filter((item) => item.id !== id);
      }
      this.#workspace.report('save', error);
      return false;
    } finally {
      this.restoring = this.restoring.filter((each) => each !== id);
    }
  }
}

/** The boards of the reader's team in the trash that they were an admin of. */
export function boardTrash(workspace: Workspace): Trash {
  return new Trash(
    workspace,
    async (paging) => {
      const team = workspace.team;
      return team ? workspace.backend.deletedBoards(team.id, paging) : [];
    },
    (boardId) => workspace.restoreBoard(boardId),
  );
}

/** The cards of a board in the trash. One that comes back is read again, and shows where it was. */
export function cardTrash(board: BoardStore): Trash {
  return new Trash(
    board.workspace,
    (paging) => board.workspace.backend.deletedCards(board.boardId, paging),
    (cardId) => board.restoreCard(cardId),
  );
}
