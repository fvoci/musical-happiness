/**
 * The data the in-memory backend works on, and the people listening to it.
 *
 * One `World` stands for one server. Several backends can share it, each
 * acting as one user: that is how a test (or the development page) lets
 * "somebody else" change a card while the reader has it open.
 */
import type { DocNode } from '../../editor/schema.ts';
import type { Block, Container, ContainerMember, BoardView, Card } from '../../logic/wire.ts';
import type {
  AccountSettings,
  Category,
  Change,
  ChangeListener,
  CommentVersion,
  Sharing,
  WorkspaceInfo,
  User,
} from '../backend.ts';

import { reads } from './board-access.ts';
import type { KeptOfCard } from './body-versions.ts';
import type { ContainerPlace } from './trash.ts';

/** A view, card, comment or attachment as the world stores it. */
export interface StoredBlock extends Block {
  /** Cards and comments: their text. */
  body?: DocNode | null;
  /** Comments whose text was changed: when that last happened. */
  editedAt?: number;
  /** Attachments. */
  file?: { fileId: string; name: string; size: number; contentType: string };
}

export type StoredView = StoredBlock & BoardView;
export type StoredCard = StoredBlock & Card;

export interface StoredFile {
  fileId: string;
  containerId: string;
  name: string;
  size: number;
  contentType: string;
  /** Absent for the files the seed only names: their bytes are not in this repository. */
  blob?: Blob;
  url?: string;
}

interface DeletedBlocks {
  containerId: string;
  /** The block that was deleted and everything that went with it. */
  blocks: StoredBlock[];
  deletedAt: number;
  deletedBy: string;
}

interface DeletedContainer {
  container: Container;
  members: ContainerMember[];
  blocks: StoredBlock[];
  deletedAt: number;
  deletedBy: string;
  /** Where each person had filed it. */
  places: ContainerPlace[];
}

interface Watcher {
  /** Undefined: listens to the reader's own things. */
  containerId: string | undefined;
  userId: string;
  listener: ChangeListener;
  /** Of a wiki: whether the watcher reads it, as of the last change told. */
  reads?: boolean;
}

export class World {
  workspace: WorkspaceInfo = { id: '', title: '' };
  readonly workspaceMembers = new Set<string>();
  readonly users = new Map<string, User>();
  readonly containers = new Map<string, Container>();
  readonly members = new Map<string, ContainerMember[]>();
  /** Public links of boards, by board. */
  readonly sharing = new Map<string, Sharing>();
  readonly blocks = new Map<string, StoredBlock>();
  readonly files = new Map<string, StoredFile>();
  readonly categories = new Map<string, Category[]>();
  readonly settings = new Map<string, AccountSettings>();
  readonly screenState = new Map<string, Record<string, string>>();
  /** Deleted blocks, by the identifier of the block the deletion was asked for. */
  readonly deletedBlocks = new Map<string, DeletedBlocks>();
  readonly deletedContainers = new Map<string, DeletedContainer>();
  /** The texts written down of changed comments, by comment, the one it has now last. */
  readonly commentVersions = new Map<string, CommentVersion[]>();
  /** The versions kept of the bodies of cards, by card (`body-versions.ts`). */
  readonly bodyVersions = new Map<string, KeptOfCard>();

  /**
   * The name of the channel the rooms of the bodies share with the worlds of
   * the browser's other tabs (`rooms.ts`); none unless set.
   */
  shareRooms: string | undefined;
  /**
   * Whether the client configuration says that bodies are searched
   * (`searchEnabled`): every server does; false shows a client that is told
   * otherwise, which has no search box.
   */
  searchEnabled = true;
  /** Before a card's body is written whole: what its room's editors wrote is written into the card (`rooms.ts`). */
  bodyFlushed: ((cardId: string) => void) | undefined;
  /** Whether a card's room holds what its body does not yet (`rooms.ts`). */
  bodyUnwritten: ((cardId: string) => boolean) | undefined;
  /** A card's body was written whole: its room, if it has one, makes its document anew (`rooms.ts`). */
  bodyWritten: ((cardId: string) => void) | undefined;

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

  blocksOf(containerId: string): StoredBlock[] {
    return [...this.blocks.values()].filter((block) => block.containerId === containerId);
  }

  childrenOf(parentId: string): StoredBlock[] {
    return [...this.blocks.values()].filter((block) => block.parentId === parentId);
  }

  watch(watcher: Watcher): () => void {
    if (watcher.containerId !== undefined)
      watcher.reads = reads(this, watcher.userId, watcher.containerId);
    this.#watchers.add(watcher);
    return () => {
      this.#watchers.delete(watcher);
    };
  }

  /**
   * Tells everybody who watches the board. Of a wiki, as the server tells it:
   * its readers are told the change; who stopped reading it with this change
   * (made private, or taken off it) is told that it is gone for them, once;
   * who does not read it is told nothing.
   */
  tellContainer(containerId: string, change: Change): void {
    if (this.containers.get(containerId)?.kind !== 'wiki') {
      this.#tell((watcher) => watcher.containerId === containerId, change);
      return;
    }
    const watching = [...this.#watchers].filter((watcher) => watcher.containerId === containerId);
    const before = new Map(watching.map((watcher) => [watcher, watcher.reads === true]));
    for (const watcher of watching) watcher.reads = reads(this, watcher.userId, containerId);
    this.#tell((watcher) => watcher.containerId === containerId && watcher.reads === true, change);
    this.#tell((watcher) => before.get(watcher) === true && watcher.reads !== true, {
      kind: 'containerDeleted',
      containerId,
    });
  }

  /** Tells the given users, wherever they are. */
  tellUsers(userIds: readonly string[], change: Change): void {
    this.#tell(
      (watcher) => watcher.containerId === undefined && userIds.includes(watcher.userId),
      change,
    );
  }

  /** Tells the user, wherever they are, that a board or wiki is gone for them, unless they still read it (an open wiki). */
  tellGone(userId: string, containerId: string): void {
    if (!reads(this, userId, containerId))
      this.tellUsers([userId], { kind: 'containerDeleted', containerId });
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
