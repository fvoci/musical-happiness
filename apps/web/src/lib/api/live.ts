/**
 * The server's changes as they happen (`contracts/websocket.md`): one
 * WebSocket per page, subscribed to the reader's team, its messages turned
 * into the screens' changes (`Change`) and handed to whoever listens to that
 * board or to the reader's own things.
 *
 * The server answers no command and does not send again what a connection
 * missed. So a connection that drops is opened again after a wait that
 * grows (it shrinks back once a connection has held), and every listener is
 * then told to read again (`resync`): what happened in between is not
 * known. The browser saying it is online again, or the reader signing in
 * (`restart`), opens it at once.
 *
 * Members and categories come in pieces, and being added to a board or
 * taken off one sends no message of the board. So a member message has the
 * board's listeners read its members, and a member or category message has
 * the reader's own listeners read the sidebar again (`resync`, gathered).
 */
import { isBackendError, type Change, type ChangeListener } from '../state/backend.ts';

import { attachmentOf, boardOf, cardOfBlock, commentOf, viewOf, type Schemas } from './convert.ts';

type Members = Extract<Change, { kind: 'members' }>['members'];
type ChangedBlock = Extract<Change, { kind: 'blocks' }>['blocks'][number];

/** What the messages do not carry, read over HTTP instead. */
export interface LiveReads {
  /** The reader's team, which the connection subscribes to. */
  teamId(): Promise<string>;
  /** A board's members: a message carries one member, the screens want them all. */
  members(boardId: string): Promise<Members>;
}

interface Listening {
  /** `undefined` for the reader's own things. */
  boardId: string | undefined;
  listener: ChangeListener;
}

/** The waits before opening again, in milliseconds, by how many tries failed; the last repeats. */
const WAITS = [500, 1000, 2000, 5000, 10_000, 30_000];
/** How long a connection holds before the waits start again from the first. */
const STEADY = 10_000;
/** How long member and category messages are gathered before the sidebar is read again. */
const GATHER = 200;

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
  #steady: ReturnType<typeof setTimeout> | undefined;
  #gathering: ReturnType<typeof setTimeout> | undefined;
  /** The latest reading of each board's members: an older answer is not told. */
  readonly #readings = new Map<string, number>();
  #readCount = 0;

  /** `url` is the address of `/ws`, with `ws:` or `wss:`. */
  constructor(url: string, reads: LiveReads) {
    this.#url = url;
    this.#reads = reads;
    globalThis.addEventListener('online', () => {
      if (this.#listening.size > 0 && !this.#socket) this.#openNow();
    });
  }

  /**
   * Opens the connection again at once, as somebody new: after signing in,
   * the cookie is another session's (an ended session's connection is
   * anonymous and hears nothing). The listeners then read again.
   */
  restart(): void {
    this.#teamId = undefined;
    // The old connection's close is not heard (it is no longer the one): its timer goes here.
    clearTimeout(this.#steady);
    const socket = this.#socket;
    this.#socket = undefined;
    socket?.close();
    if (this.#listening.size === 0) return;
    this.#wasOpen = true;
    this.#openNow();
  }

  #openNow(): void {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    void this.#open();
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
    clearTimeout(this.#steady);
    clearTimeout(this.#gathering);
    this.#timer = undefined;
    this.#gathering = undefined;
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
      // A connection that is closed as soon as it opens does not count as held.
      this.#steady = setTimeout(() => {
        this.#failures = 0;
      }, STEADY);
      // The subscription has no answer: a write between this and the
      // server taking it in is missed. The window is short.
      if (this.#wasOpen) this.#tellAll({ kind: 'resync' });
      this.#wasOpen = true;
    });
    socket.addEventListener('message', (event: MessageEvent<unknown>) => {
      if (this.#socket !== socket || typeof event.data !== 'string') return;
      void this.#receive(event.data);
    });
    socket.addEventListener('close', () => {
      if (this.#socket !== socket) return;
      clearTimeout(this.#steady);
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
      this.#sidebarSoon();
      await this.#readMembers(message.member.boardId);
    } else if (message.action.includes('CATEGOR')) {
      this.#sidebarSoon();
    }
  }

  /**
   * Reads a board's members for its listeners, if it has any. Of two
   * readings, only the later one is told. A board the reader may no longer
   * read (taken off it) is gone for them.
   */
  async #readMembers(boardId: string): Promise<void> {
    if (![...this.#listening].some((entry) => entry.boardId === boardId)) return;
    this.#readCount += 1;
    const reading = this.#readCount;
    this.#readings.set(boardId, reading);
    try {
      const members = await this.#reads.members(boardId);
      if (this.#readings.get(boardId) !== reading) return;
      this.#tellBoard(boardId, { kind: 'members', boardId, members }, false);
    } catch (error) {
      if (this.#readings.get(boardId) !== reading) return;
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.#tellBoard(boardId, { kind: 'boardDeleted', boardId }, false);
      }
      // Anything else: the next message, or the next reading, brings them.
    }
  }

  /** Has the reader's own listeners read the sidebar again, once for messages close together. */
  #sidebarSoon(): void {
    if (this.#gathering !== undefined) return;
    this.#gathering = setTimeout(() => {
      this.#gathering = undefined;
      this.#tellSelf({ kind: 'resync' });
    }, GATHER);
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
