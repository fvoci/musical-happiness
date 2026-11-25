/**
 * The server's changes as they happen (`contracts/websocket.md`): one
 * WebSocket per page, subscribed to the workspace the reader is in
 * (`enter`: the one the address names), its messages turned
 * into the screens' changes (`Change`) and handed to whoever listens to that
 * board or to the reader's own things.
 *
 * The server answers no command and does not send again what a connection
 * missed. So a connection that drops is opened again after a wait that
 * grows (it shrinks back once a connection has held), and every listener is
 * then told to read again (`resync`): what happened in between is not
 * known. The browser saying it is online again, or the reader signing in
 * (`restart`), opens it at once; the browser saying it is offline drops it
 * at once, and so does a `ping` nothing answers in time (`live-heartbeat.ts`:
 * a connection that died says nothing for a long while otherwise). Nothing
 * is opened before a workspace is entered; entering another one moves the
 * subscription over to it.
 *
 * Members and categories come in pieces, and being added to a board or
 * taken off one sends no message of the board. So a member message has the
 * board's listeners read its members, and a member or category message has
 * the reader's own listeners read the sidebar again (`resync`, gathered).
 *
 * Access taken back ends the subscription, not the connection
 * (`workspaceUnsubscribed`): it is asked again, which takes the new role or
 * is refused, and the listeners read again.
 *
 * What the reader was told changed (`notificationsChanged`, whatever
 * workspace is followed) is told to the reader's own listeners, who read it
 * again.
 *
 * The rooms of the bodies edited together use the same connection
 * (`live-rooms.ts`): an open room keeps it open as a listener does, and
 * hears when it comes and goes. Their binary frames come on it too.
 */
import { isBackendError, type Change, type ChangeListener } from '../state/backend.ts';
import type { BodySummary } from '../state/body-summary.ts';

import {
  attachmentOf,
  containerOf,
  cardOfBlock,
  commentOf,
  viewOf,
  type Schemas,
} from './convert.ts';
import { Heartbeat, Patience, PING, PONG } from './live-heartbeat.ts';
import { LivePresence } from './live-presence.ts';
import { LiveRooms } from './live-rooms.ts';
import { pageOfBlock } from './wikis.ts';
import { entryOfBlock } from './diaries.ts';
import { unitOfBlock } from './courses.ts';

type Members = Extract<Change, { kind: 'members' }>['members'];
type ChangedBlock = Extract<Change, { kind: 'blocks' }>['blocks'][number];

/** What the messages do not carry, read over HTTP instead. */
export interface LiveReads {
  /** A board's or a wiki's members: a message carries one member, the screens want them all. */
  members(containerId: string): Promise<Members>;
}

interface Listening {
  /** `undefined` for the reader's own things. */
  containerId: string | undefined;
  listener: ChangeListener;
}

/** The waits before opening again, in milliseconds, by how many tries failed; the last repeats. */
const WAITS = [500, 1000, 2000, 5000, 10_000, 30_000];
/**
 * The close the server sends when it could not read what a connection may be told
 * (`contracts/websocket.md` 3): every page it closed so comes back, so each waits
 * a random part of its wait more, that they do not come back all at once.
 */
const TRY_AGAIN = 1013;
/** How long a connection holds before the waits start again from the first. */
const STEADY = 10_000;
/** How long member and category messages are gathered before the sidebar is read again. */
const GATHER = 200;

const CATEGORY_MESSAGE_TYPES = new Set([
  'categoryChanged',
  'containerCategoriesChanged',
  'categoriesReordered',
  'categoryContainersReordered',
]);

type Message = { workspaceId?: string } & (
  | { type: 'containerChanged'; container: Schemas['Container'] }
  | { type: 'blockChanged'; block: Schemas['Block']; summary?: BodySummary }
  | { type: 'containerMemberChanged'; member: Schemas['ContainerMember'] }
  | {
      type: 'containerMemberRemoved';
      member: { containerId: string; userId: string };
    }
  | { type: 'userChanged'; userId: string }
  | { type: string }
);

export class LiveChanges {
  readonly #url: string;
  readonly #reads: LiveReads;
  readonly #listening = new Set<Listening>();
  #socket: WebSocket | undefined;
  /** Whether `#socket` still carries, once it opened. */
  #heartbeat: Heartbeat | undefined;
  /** How long the connections' `ping`s wait. */
  readonly #patience = new Patience();
  /** Whether `#socket` has opened and subscribed. */
  #subscribed = false;
  /** The workspace entered: the one subscribed to. */
  #workspaceId: string | undefined;
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
  /** The rooms of the bodies open on this connection. */
  readonly rooms: LiveRooms;
  readonly presence: LivePresence;

  /** `url` is the address of `/ws`, with `ws:` or `wss:`. */
  constructor(url: string, reads: LiveReads) {
    this.#url = url;
    this.#reads = reads;
    this.rooms = new LiveRooms(
      {
        text: (command) => this.#send(JSON.stringify(command)),
        bytes: (frame) => this.#send(frame),
      },
      () => {
        if (!this.#wanted()) this.#stop();
        else if (!this.#socket && this.#timer === undefined) this.#open();
      },
    );
    this.presence = new LivePresence(
      (command) => {
        this.#send(JSON.stringify(command));
      },
      () => {
        if (!this.#wanted()) this.#stop();
        else if (!this.#socket && this.#timer === undefined) this.#open();
      },
    );
    globalThis.addEventListener('online', () => {
      if (this.#wanted() && !this.#socket) this.#openNow();
    });
    globalThis.addEventListener('offline', () => {
      const socket = this.#socket;
      if (socket) this.#letGo(socket);
    });
  }

  /**
   * Opens the connection again at once, as somebody new: after signing in,
   * the cookie is another session's (an ended session's connection is
   * anonymous and hears nothing). The listeners then read again.
   */
  restart(): void {
    this.#failures = 0;
    // The old connection's close is not heard (it is no longer the one): its timer goes here.
    clearTimeout(this.#steady);
    this.#stopHeartbeat();
    const socket = this.#socket;
    this.#socket = undefined;
    this.#subscribed = false;
    socket?.close();
    this.rooms.down();
    this.presence.down();
    if (!this.#wanted()) return;
    this.#wasOpen = true;
    this.#openNow();
  }

  #openNow(): void {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#open();
  }

  /**
   * Follows `workspaceId` from now on. An open connection leaves the one it
   * followed and subscribes to this one; what was heard of the old one is
   * the business of whoever listened to it (the workspace reads anew).
   *
   * Entered again (`Workspace.enter`, which reads it anew), the one it follows
   * is asked for again: the server says nothing of an asking it ignores (the
   * own space, until the session has shown the account's password, on this
   * connection or on the one before it), so the page cannot know that it is
   * followed, and the server takes an asking for what is followed already.
   * Whoever listens reads again too (`resync`): what was not heard meanwhile is
   * not only what `Workspace.enter` reads (the titles of what bodies refer to).
   */
  enter(workspaceId: string): void {
    const before = this.#workspaceId;
    if (before === workspaceId) {
      this.#send(JSON.stringify({ type: 'subscribeWorkspace', workspaceId }));
      this.#tellAll({ kind: 'resync' });
      return;
    }
    this.#workspaceId = workspaceId;
    const socket = this.#socket;
    if (socket && this.#subscribed) {
      if (before !== undefined) {
        socket.send(JSON.stringify({ type: 'unsubscribeWorkspace', workspaceId: before }));
      }
      socket.send(JSON.stringify({ type: 'subscribeWorkspace', workspaceId }));
    } else if (!socket && this.#wanted() && this.#timer === undefined) {
      this.#open();
    }
  }

  watch(containerId: string | undefined, listener: ChangeListener): () => void {
    const entry: Listening = { containerId, listener };
    this.#listening.add(entry);
    if (!this.#socket && this.#timer === undefined) this.#open();
    return () => {
      this.#listening.delete(entry);
      if (!this.#wanted()) this.#stop();
    };
  }

  /** Somebody listens, or a room is open: the connection is kept. */
  #wanted(): boolean {
    return this.#listening.size > 0 || this.rooms.size > 0 || this.presence.wanted;
  }

  /** Sends on the connection if it is open and subscribed; false when it is not. */
  #send(data: string | Uint8Array<ArrayBuffer>): boolean {
    const socket = this.#socket;
    if (!socket || !this.#subscribed) return false;
    socket.send(data);
    return true;
  }

  #stop(): void {
    clearTimeout(this.#timer);
    clearTimeout(this.#steady);
    clearTimeout(this.#gathering);
    this.#timer = undefined;
    this.#gathering = undefined;
    this.#stopHeartbeat();
    const socket = this.#socket;
    this.#socket = undefined;
    this.#subscribed = false;
    this.#wasOpen = false;
    this.#failures = 0;
    socket?.close();
    this.rooms.down();
    this.presence.down();
  }

  #open(): void {
    // Nothing to follow yet: entering a workspace opens it.
    if (this.#workspaceId === undefined) return;
    if (!this.#wanted() || this.#socket) return;
    const socket = new WebSocket(this.#url);
    socket.binaryType = 'arraybuffer';
    this.#socket = socket;
    this.#subscribed = false;
    socket.addEventListener('open', () => {
      if (this.#socket !== socket) return;
      socket.send(JSON.stringify({ type: 'subscribeWorkspace', workspaceId: this.#workspaceId }));
      this.#subscribed = true;
      this.#heartbeat = new Heartbeat(
        () => {
          socket.send(PING);
        },
        () => {
          this.#letGo(socket);
        },
        this.#patience,
      );
      // A connection that is closed as soon as it opens does not count as held.
      this.#steady = setTimeout(() => {
        this.#failures = 0;
      }, STEADY);
      // The subscription has no answer: a write between this and the
      // server taking it in is missed. The window is short.
      if (this.#wasOpen) this.#tellAll({ kind: 'resync' });
      this.#wasOpen = true;
      this.rooms.up();
      this.presence.up();
    });
    socket.addEventListener('message', (event: MessageEvent<unknown>) => {
      if (this.#socket !== socket) return;
      this.#heartbeat?.heard();
      if (event.data instanceof ArrayBuffer) this.rooms.bytes(new Uint8Array(event.data));
      else if (typeof event.data === 'string' && event.data !== PONG)
        void this.#receive(event.data);
    });
    socket.addEventListener('close', (event: CloseEvent) => {
      this.#dropped(socket, event.code === TRY_AGAIN);
    });
  }

  /**
   * The connection `socket` is gone, if it was the one: the rooms and the
   * visits hear, and it is opened again after a wait (`spread`: the server
   * asked to try again).
   */
  #dropped(socket: WebSocket, spread = false): void {
    if (this.#socket !== socket) return;
    clearTimeout(this.#steady);
    this.#stopHeartbeat();
    this.#socket = undefined;
    this.#subscribed = false;
    this.#failures += 1;
    this.rooms.down();
    this.presence.down();
    this.#again(spread);
  }

  /**
   * Gives up the connection `socket` that is taken for gone (the browser is
   * offline, a `PING` was not answered): its close may not be heard for long.
   */
  #letGo(socket: WebSocket): void {
    this.#dropped(socket);
    socket.close();
  }

  #stopHeartbeat(): void {
    this.#heartbeat?.stop();
    this.#heartbeat = undefined;
  }

  /** Opens again after the wait of the failures so far; `spread` adds a random part of it. */
  #again(spread = false): void {
    if (!this.#wanted() || this.#timer !== undefined) return;
    const base = WAITS[Math.min(this.#failures - 1, WAITS.length - 1)] ?? 30_000;
    const wait = spread ? base + Math.floor(Math.random() * base) : base;
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      this.#open();
    }, wait);
  }

  #tellAll(change: Change): void {
    for (const { listener } of [...this.#listening]) listener(change);
  }

  /** To the listeners of `containerId`, and to those of the reader's own things when `self`. */
  #tellContainer(containerId: string, change: Change, self: boolean): void {
    for (const { containerId: watched, listener } of [...this.#listening]) {
      if (watched === containerId || (self && watched === undefined)) listener(change);
    }
  }

  #tellSelf(change: Change): void {
    for (const { containerId, listener } of [...this.#listening]) {
      if (containerId === undefined) listener(change);
    }
  }

  async #receive(text: string): Promise<void> {
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch {
      return;
    }
    if (typeof parsed !== 'object' || parsed === null) return;
    if (this.rooms.text(parsed as Record<string, unknown>)) return;
    if (this.presence.text(parsed as Record<string, unknown>)) return;
    const message = parsed as Message;
    if (typeof message.type !== 'string') return;
    // Still on its way from a workspace that was left.
    if (message.workspaceId !== undefined && message.workspaceId !== this.#workspaceId) return;
    if (message.type === 'workspaceUnsubscribed') {
      this.#send(JSON.stringify({ type: 'subscribeWorkspace', workspaceId: this.#workspaceId }));
      this.#tellAll({ kind: 'resync' });
      return;
    }
    if (message.type === 'notificationsChanged') {
      this.#tellSelf({ kind: 'notifications' });
    } else if (message.type === 'userChanged' && 'userId' in message) {
      this.#tellSelf({ kind: 'user', userId: message.userId });
    } else if (message.type === 'containerChanged' && 'container' in message) {
      const { container } = message;
      // A deleted board or wiki carries only its identifier and times.
      const change: Change =
        container.deletedAt === undefined
          ? { kind: 'container', container: containerOf(container) }
          : { kind: 'containerDeleted', containerId: container.id };
      this.#tellContainer(container.id, change, true);
    } else if (message.type === 'blockChanged' && 'block' in message) {
      const { block, summary } = message;
      if (block.deletedAt !== undefined) {
        // Only the block deleted is told, not what went with it.
        this.#tellContainer(
          block.containerId,
          { kind: 'blocksDeleted', containerId: block.containerId, blockIds: [block.id] },
          false,
        );
        return;
      }
      if (block.type === 'page') {
        this.#tellContainer(
          block.containerId,
          { kind: 'pages', containerId: block.containerId, pages: [pageOfBlock(block)] },
          false,
        );
        return;
      }
      if (block.type === 'entry') {
        this.#tellContainer(
          block.containerId,
          { kind: 'entries', containerId: block.containerId, entries: [entryOfBlock(block)] },
          false,
        );
        return;
      }
      if (block.type === 'unit' || block.type === 'chapter') {
        this.#tellContainer(
          block.containerId,
          { kind: 'units', containerId: block.containerId, units: [unitOfBlock(block)] },
          false,
        );
        return;
      }
      const changed = changedBlock(block, summary);
      if (changed) {
        this.#tellContainer(
          block.containerId,
          { kind: 'blocks', containerId: block.containerId, blocks: [changed] },
          false,
        );
      }
    } else if (
      (message.type === 'containerMemberChanged' || message.type === 'containerMemberRemoved') &&
      'member' in message
    ) {
      this.#sidebarSoon();
      await this.#readMembers(message.member.containerId);
    } else if (CATEGORY_MESSAGE_TYPES.has(message.type)) {
      this.#sidebarSoon();
    }
  }

  /**
   * Reads a board's members for its listeners, if it has any. Of two
   * readings, only the later one is told. A board the reader may no longer
   * read (taken off it) is gone for them.
   */
  async #readMembers(containerId: string): Promise<void> {
    if (![...this.#listening].some((entry) => entry.containerId === containerId)) return;
    this.#readCount += 1;
    const reading = this.#readCount;
    this.#readings.set(containerId, reading);
    try {
      const members = await this.#reads.members(containerId);
      if (this.#readings.get(containerId) !== reading) return;
      this.#tellContainer(containerId, { kind: 'members', containerId, members }, false);
    } catch (error) {
      if (this.#readings.get(containerId) !== reading) return;
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.#tellContainer(containerId, { kind: 'containerDeleted', containerId }, false);
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

/**
 * A block of a message as the screens' changed block; other kinds are not
 * shown. A card's message carries the summary of its body in place of it.
 */
function changedBlock(
  block: Schemas['Block'],
  summary: BodySummary | undefined,
): ChangedBlock | undefined {
  switch (block.type) {
    case 'view':
      return viewOf(block);
    case 'card':
      return summary && cardOfBlock(block, summary);
    case 'comment':
      return commentOf(block);
    case 'attachment':
      return attachmentOf(block);
    default:
      return undefined;
  }
}
