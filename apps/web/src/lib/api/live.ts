/**
 * The server's changes as they happen (`contracts/websocket.md`): one
 * WebSocket per page, subscribed to the reader's team, its messages turned
 * into the screens' changes (`Change`) and handed to whoever listens to that
 * board or to the reader's own things.
 *
 * The server answers no command and does not send again what a connection
 * missed. So a connection that drops is opened again after a wait that
 * grows, and every listener is then told to read again (`resync`): what
 * happened in between is not known.
 */
import type { Change, ChangeListener } from '../state/backend.ts';

import { attachmentOf, boardOf, cardOfBlock, commentOf, viewOf, type Schemas } from './convert.ts';

type Members = Extract<Change, { kind: 'members' }>['members'];
type Categories = Extract<Change, { kind: 'categories' }>['categories'];
type ChangedBlock = Extract<Change, { kind: 'blocks' }>['blocks'][number];

/** What the messages do not carry, read over HTTP instead. */
export interface LiveReads {
  /** The reader's team, which the connection subscribes to. */
  teamId(): Promise<string>;
  /** A board's members: a message carries one member, the screens want them all. */
  members(boardId: string): Promise<Members>;
  /** The reader's categories: the messages carry pieces of them. */
  categories(teamId: string): Promise<Categories>;
}

interface Listening {
  /** `undefined` for the reader's own things. */
  boardId: string | undefined;
  listener: ChangeListener;
}

/** The waits before opening again, in milliseconds, by how many tries failed; the last repeats. */
const WAITS = [500, 1000, 2000, 5000, 10_000, 30_000];

type Message =
  | { action: 'UPDATE_BOARD'; board: Schemas['Board'] }
  | { action: 'UPDATE_BLOCK'; block: Schemas['Block'] }
  | { action: 'UPDATE_MEMBER' | 'DELETE_MEMBER'; member: Schemas['BoardMember'] }
  | { action: string };

export class LiveChanges {
  readonly #url: string;
  readonly #reads: LiveReads;
  readonly #listening = new Set<Listening>();
  #socket: WebSocket | undefined;
  #teamId: string | undefined;
  /** Tries in a row that did not open: picks the wait. */
  #failures = 0;
  /** A connection was open before: the next one that opens is a reconnection. */
  #wasOpen = false;
  #timer: ReturnType<typeof setTimeout> | undefined;

  /** `url` is the address of `/ws`, with `ws:` or `wss:`. */
  constructor(url: string, reads: LiveReads) {
    this.#url = url;
    this.#reads = reads;
  }

  watch(boardId: string | undefined, listener: ChangeListener): () => void {
    const entry: Listening = { boardId, listener };
    this.#listening.add(entry);
    if (!this.#socket && this.#timer === undefined) void this.#open();
    return () => {
      this.#listening.delete(entry);
      if (this.#listening.size === 0) this.#stop();
    };
  }

  #stop(): void {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    const socket = this.#socket;
    this.#socket = undefined;
    this.#wasOpen = false;
    this.#failures = 0;
    socket?.close();
  }

  async #open(): Promise<void> {
    try {
      this.#teamId ??= await this.#reads.teamId();
    } catch {
      this.#failures += 1;
      this.#again();
      return;
    }
    const teamId = this.#teamId;
    if (this.#listening.size === 0 || this.#socket) return;
    const socket = new WebSocket(this.#url);
    this.#socket = socket;
    socket.addEventListener('open', () => {
      if (this.#socket !== socket) return;
      socket.send(JSON.stringify({ action: 'SUBSCRIBE_TEAM', teamId }));
      this.#failures = 0;
      if (this.#wasOpen) this.#tellAll({ kind: 'resync' });
      this.#wasOpen = true;
    });
    socket.addEventListener('message', (event: MessageEvent<unknown>) => {
      if (this.#socket !== socket || typeof event.data !== 'string') return;
      void this.#receive(event.data);
    });
    socket.addEventListener('close', () => {
      if (this.#socket !== socket) return;
      this.#socket = undefined;
      this.#failures += 1;
      this.#again();
    });
  }

  #again(): void {
    if (this.#listening.size === 0 || this.#timer !== undefined) return;
    const wait = WAITS[Math.min(this.#failures - 1, WAITS.length - 1)] ?? 30_000;
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      void this.#open();
    }, wait);
  }

  #tellAll(change: Change): void {
    for (const { listener } of [...this.#listening]) listener(change);
  }

  /** To the listeners of `boardId`, and to those of the reader's own things when `self`. */
  #tellBoard(boardId: string, change: Change, self: boolean): void {
    for (const { boardId: watched, listener } of [...this.#listening]) {
      if (watched === boardId || (self && watched === undefined)) listener(change);
    }
  }

  #tellSelf(change: Change): void {
    for (const { boardId, listener } of [...this.#listening]) {
      if (boardId === undefined) listener(change);
    }
  }

  async #receive(text: string): Promise<void> {
    let message: Message;
    try {
      message = JSON.parse(text) as Message;
    } catch {
      return;
    }
    if ('board' in message) {
      const { board } = message;
      // A deleted board carries only its identifier and times.
      const change: Change =
        board.deleteAt === 0
          ? { kind: 'board', board: boardOf(board) }
          : { kind: 'boardDeleted', boardId: board.id };
      this.#tellBoard(board.id, change, true);
    } else if ('block' in message) {
      const { block } = message;
      if (block.deleteAt !== 0) {
        // Only the block deleted is told, not what went with it.
        this.#tellBoard(
          block.boardId,
          { kind: 'blocksDeleted', boardId: block.boardId, blockIds: [block.id] },
          false,
        );
        return;
      }
      const changed = changedBlock(block);
      if (changed) {
        this.#tellBoard(
          block.boardId,
          { kind: 'blocks', boardId: block.boardId, blocks: [changed] },
          false,
        );
      }
    } else if ('member' in message) {
      const { boardId } = message.member;
      try {
        this.#tellBoard(
          boardId,
          { kind: 'members', boardId, members: await this.#reads.members(boardId) },
          false,
        );
      } catch {
        // Not a member any more (403): the reader no longer reads this board.
      }
    } else if (message.action.includes('CATEGOR') && this.#teamId) {
      try {
        this.#tellSelf({
          kind: 'categories',
          categories: await this.#reads.categories(this.#teamId),
        });
      } catch {
        // The next change, or the next reading, brings them.
      }
    }
  }
}

/** A block of a message as the screens' changed block; other kinds are not shown. */
function changedBlock(block: Schemas['Block']): ChangedBlock | undefined {
  switch (block.type) {
    case 'view':
      return viewOf(block);
    case 'card':
      return cardOfBlock(block);
    case 'comment':
      return commentOf(block);
    case 'attachment':
      return attachmentOf(block);
    default:
      return undefined;
  }
}
