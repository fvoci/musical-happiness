/**
 * The inbox of the top bar: its count follows what the reader is told as it
 * happens, the list is read while it is open, who did each is named, and
 * reading one or all of them shows at once.
 */
import { describe, expect, it } from 'vitest';

import { NotificationInbox } from '$lib/state/notification-inbox.svelte.ts';

import { settle, setup, text } from './support.ts';

async function anInbox() {
  const made = setup();
  const [workspace] = await made.backend.workspaces();
  const [board] = await made.backend.boards(workspace?.id ?? '');
  if (!board) throw new Error('no board');
  const card = await made.backend.createCard(board.id, { title: 'Plan' });
  const refused: unknown[] = [];
  const inbox = new NotificationInbox(made.backend.notifications, made.backend, (error) =>
    refused.push(error),
  );
  const comment = (words: string) => made.other.createComment(board.id, card.id, text(words));
  return { ...made, inbox, refused, comment };
}

describe('the inbox', () => {
  it('counts what the reader is told as it happens, and lists it while open', async () => {
    const made = await anInbox();
    const stop = made.inbox.start();
    await settle();
    expect(made.inbox.unread).toBe(0);

    await made.comment('first');
    await settle();
    await settle();
    expect(made.inbox.unread).toBe(1);
    expect(made.inbox.listed).toBe(false);

    await made.inbox.open();
    await settle();
    expect(made.inbox.items).toHaveLength(1);
    expect(made.inbox.people.get(made.other.userId)?.id).toBe(made.other.userId);

    // Open, the list follows: more of the same card joins its line.
    await made.comment('second');
    await settle();
    await settle();
    expect(made.inbox.items.map((one) => one.subject?.times)).toEqual([2]);
    stop();
    expect(made.refused).toEqual([]);
  });

  it('shows one read, and all read, at once', async () => {
    const made = await anInbox();
    made.inbox.start();
    await made.comment('to read');
    await made.inbox.open();
    const [first] = made.inbox.items;
    if (!first) throw new Error('listed');
    const reading = made.inbox.read(first);
    expect(made.inbox.items[0]?.readAt).toBeGreaterThan(0);
    await reading;
    await settle();
    expect(made.inbox.unread).toBe(0);

    await made.comment('again');
    await settle();
    await settle();
    expect(made.inbox.unread).toBe(1);
    await made.inbox.readAll();
    await settle();
    expect(made.inbox.unread).toBe(0);
    expect(made.inbox.items.every((one) => one.readAt > 0)).toBe(true);
  });
});
