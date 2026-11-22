/**
 * The reader's notifications as the top bar shows them: how many are unread,
 * always, and the newest of them while the list is open. Both are read again
 * when the backend says that what the reader was told changed
 * (`notifications`: something was told, or read, in this tab or another) and
 * when changes may have been missed (`resync`); the list only while it is
 * open. Of two readings, the later one counts.
 *
 * Who did each is named as the workspace of its card or page names them
 * (`usersById`), asked once a visit; whom it leaves out stays unnamed.
 */
import { SvelteMap } from 'svelte/reactivity';

import type { Backend, User } from './backend.ts';
import type { Notification, Notifications } from './notification-types.ts';

/** How many the list shows: the newest. */
export const LISTED = 50;

type InboxBackend = Pick<Backend, 'watchSelf' | 'usersById'>;

export class NotificationInbox {
  /** How many the reader has not read. */
  unread = $state(0);
  /** The newest, while the list is open; empty before it was read. */
  items = $state.raw<readonly Notification[]>([]);
  /** Whether the list was read since it was opened. */
  listed = $state(false);
  /** The people named so far, by identifier. */
  readonly people = new SvelteMap<string, User>();

  readonly #notifications: Notifications;
  readonly #backend: InboxBackend;
  readonly #refused: (error: unknown) => void;
  #open = false;
  #countReading = 0;
  #listReading = 0;
  /** Who was asked for already, as `workspace/user`. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- nothing that is drawn reads it
  readonly #asked = new Set<string>();

  /** `refused` hears what could not be read or written: a session that is over, say. */
  constructor(
    notifications: Notifications,
    backend: InboxBackend,
    refused: (error: unknown) => void,
  ) {
    this.#notifications = notifications;
    this.#backend = backend;
    this.#refused = refused;
  }

  /** Reads the count, and listens from now on; the function that stops listening. */
  start(): () => void {
    void this.#readCount();
    return this.#backend.watchSelf((change) => {
      if (change.kind === 'notifications' || change.kind === 'resync') void this.refresh();
    });
  }

  /** Reads the count again, and the list where it is open. */
  async refresh(): Promise<void> {
    await Promise.all([this.#readCount(), this.#open ? this.#readList() : undefined]);
  }

  /** The list is shown: it is read, and kept up to date until it is closed. */
  async open(): Promise<void> {
    this.#open = true;
    await this.#readList();
  }

  close(): void {
    this.#open = false;
  }

  /** Marks one as read, on the screen at once. */
  async read(notification: Notification): Promise<void> {
    if (notification.readAt !== 0) return;
    const now = Date.now();
    this.items = this.items.map((one) =>
      one.id === notification.id ? { ...one, readAt: now } : one,
    );
    this.unread = Math.max(0, this.unread - 1);
    try {
      await this.#notifications.read(notification.id);
    } catch (error) {
      this.#refused(error);
      await this.refresh();
    }
  }

  /**
   * Marks as read all of those listed and what came before them: what
   * happened after the newest one shown stays unread.
   */
  async readAll(): Promise<void> {
    const through = this.items.reduce<number | undefined>(
      (newest, one) => Math.max(newest ?? 0, one.updateAt),
      undefined,
    );
    const now = Date.now();
    this.items = this.items.map((one) => (one.readAt === 0 ? { ...one, readAt: now } : one));
    try {
      await this.#notifications.readAll(through);
    } catch (error) {
      this.#refused(error);
    }
    await this.refresh();
  }

  async #readCount(): Promise<void> {
    this.#countReading += 1;
    const reading = this.#countReading;
    try {
      const unread = await this.#notifications.unread();
      if (reading === this.#countReading) this.unread = unread;
    } catch (error) {
      if (reading === this.#countReading) this.#refused(error);
    }
  }

  async #readList(): Promise<void> {
    this.#listReading += 1;
    const reading = this.#listReading;
    try {
      const items = await this.#notifications.list({ page: 0, perPage: LISTED });
      if (reading !== this.#listReading || !this.#open) return;
      this.items = items;
      this.listed = true;
      void this.#name(items);
    } catch (error) {
      if (reading === this.#listReading) this.#refused(error);
    }
  }

  /** Asks the workspace of each for the people the list names that are not known yet. */
  async #name(items: readonly Notification[]): Promise<void> {
    const wanted: Record<string, string[]> = {};
    for (const { subject } of items) {
      if (!subject || subject.actorId === '' || this.people.has(subject.actorId)) continue;
      const key = `${subject.workspaceId}/${subject.actorId}`;
      if (this.#asked.has(key)) continue;
      this.#asked.add(key);
      (wanted[subject.workspaceId] ??= []).push(subject.actorId);
    }
    await Promise.all(
      Object.entries(wanted).map(async ([workspaceId, ids]) => {
        try {
          const found = await this.#backend.usersById(workspaceId, ids);
          for (const user of found) this.people.set(user.id, user);
        } catch {
          // Unnamed this time: asked again when the list is read again.
          for (const id of ids) this.#asked.delete(`${workspaceId}/${id}`);
        }
      }),
    );
  }
}
