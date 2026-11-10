/**
 * The data the in-memory backend works on, and the people listening to it.
 *
 * One `World` stands for one server. Several backends can share it, each
 * acting as one user: that is how a test (or the development page) lets
 * "somebody else" change a card while the reader has it open.
 */
import type { DocNode } from '../../editor/schema.ts';
import type { Block, Board, BoardMember, BoardView, Card } from '../../logic/wire.ts';
import type { AccountSettings, Category, Change, ChangeListener, Team, User } from '../backend.ts';

/** A view, card, comment or attachment as the world stores it. */
export interface StoredBlock extends Block {
  /** Cards and comments: their text. */
  body?: DocNode | null;
  /** Attachments. */
  file?: { fileId: string; name: string; size: number; contentType: string };
}

export type StoredView = StoredBlock & BoardView;
export type StoredCard = StoredBlock & Card;

export interface StoredFile {
  fileId: string;
  boardId: string;
  name: string;
  size: number;
  contentType: string;
  /** Absent for the files the seed only names: their bytes are not in this repository. */
  blob?: Blob;
  url?: string;
}

interface DeletedBlocks {
  boardId: string;
  /** The block that was deleted and everything that went with it. */
  blocks: StoredBlock[];
}

interface DeletedBoard {
  board: Board;
  members: BoardMember[];
  blocks: StoredBlock[];
}

interface Watcher {
  /** Undefined: listens to the reader's own things. */
  boardId: string | undefined;
  userId: string;
  listener: ChangeListener;
}

export class World {
  team: Team = { id: '', title: '' };
  readonly users = new Map<string, User>();
  readonly boards = new Map<string, Board>();
  readonly members = new Map<string, BoardMember[]>();
  readonly blocks = new Map<string, StoredBlock>();
  readonly files = new Map<string, StoredFile>();
  readonly categories = new Map<string, Category[]>();
  readonly settings = new Map<string, AccountSettings>();
  readonly screenState = new Map<string, Record<string, string>>();
  /** Deleted blocks, by the identifier of the block the deletion was asked for. */
  readonly deletedBlocks = new Map<string, DeletedBlocks>();
  readonly deletedBoards = new Map<string, DeletedBoard>();

  readonly #watchers = new Set<Watcher>();
  #counter = 0;
  #clock: () => number;
  #lastTime = 0;

  constructor(clock: () => number = Date.now) {
    this.#clock = clock;
  }

  /** Replaces the clock: a seeded world is built on a made-up one and then joins real time. */
  useClock(clock: () => number): void {
    this.#clock = clock;
  }

  /**
   * An identifier the way the server makes them: a UUID of version 7 in
   * shape. The counter keeps them in the order they were made and makes a
   * seeded world come out the same every time.
   */
  newId(): string {
    this.#counter += 1;
    return `01930000-0000-7000-8000-${this.#counter.toString(16).padStart(12, '0')}`;
  }

  /** The current time in milliseconds, never the same twice: versions are told apart by it. */
  now(): number {
    const time = Math.max(this.#clock(), this.#lastTime + 1);
    this.#lastTime = time;
    return time;
  }

  blocksOf(boardId: string): StoredBlock[] {
    return [...this.blocks.values()].filter((block) => block.boardId === boardId);
  }

  childrenOf(parentId: string): StoredBlock[] {
    return [...this.blocks.values()].filter((block) => block.parentId === parentId);
  }

  watch(watcher: Watcher): () => void {
    this.#watchers.add(watcher);
    return () => {
      this.#watchers.delete(watcher);
    };
  }

  /** Tells everybody who watches the board. */
  tellBoard(boardId: string, change: Change): void {
    this.#tell((watcher) => watcher.boardId === boardId, change);
  }

  /** Tells the given users, wherever they are. */
  tellUsers(userIds: readonly string[], change: Change): void {
    this.#tell(
      (watcher) => watcher.boardId === undefined && userIds.includes(watcher.userId),
      change,
    );
  }

  #tell(wanted: (watcher: Watcher) => boolean, change: Change): void {
    const listeners = [...this.#watchers].filter(wanted).map((watcher) => watcher.listener);
    if (listeners.length === 0) return;
    // After the write has answered, like a message on a socket would be. Each
    // listener gets a copy of its own: nobody holds the world's objects.
    queueMicrotask(() => {
      for (const listener of listeners) listener(structuredClone(change));
    });
  }
}
