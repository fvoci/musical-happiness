/**
 * The bell (features/notifications/NotificationBell.svelte) in a simulated
 * page: the count on it, and when "Mark all as read" may be pressed: only
 * once the list is read and while one of it is unread. Pressed, it marks
 * what is shown, and what came before it, as read.
 */
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import NotificationBell from '$lib/features/notifications/NotificationBell.svelte';
import { NotificationInbox } from '$lib/state/notification-inbox.svelte.ts';
import type { Notification, Notifications } from '$lib/state/notification-types.ts';

afterEach(() => {
  cleanup();
});

const told = (id: string, updateAt: number, readAt = 0): Notification => ({
  id,
  createAt: updateAt,
  updateAt,
  readAt,
  subject: {
    kind: 'mention',
    actorId: '',
    times: 1,
    people: 1,
    workspaceId: 'w',
    containerId: 'b',
    containerKind: 'board',
    blockId: `card-${id}`,
    blockKind: 'card',
    title: `Card ${id}`,
    commentId: '',
  },
});

/** A bell over notifications whose list answers `items` once `answer` is called. */
function mount(items: Notification[]) {
  let answer: () => void = () => undefined;
  const listed = new Promise<Notification[]>((resolve) => {
    answer = () => {
      resolve(items);
    };
  });
  const notifications = {
    list: vi.fn<Notifications['list']>(() => listed),
    unread: vi.fn<Notifications['unread']>(() =>
      Promise.resolve(items.filter((one) => one.readAt === 0).length),
    ),
    read: vi.fn<Notifications['read']>(() => Promise.resolve()),
    readAll: vi.fn<Notifications['readAll']>(() => Promise.resolve()),
  } satisfies Notifications;
  const inbox = new NotificationInbox(
    notifications,
    { watchSelf: () => () => undefined, usersById: () => Promise.resolve([]) },
    () => undefined,
  );
  render(NotificationBell, {
    props: { inbox, locale: 'en-US', hrefOf: () => '/somewhere', onGo: vi.fn() },
  });
  return { notifications, inbox, answer };
}

const readAll = (): HTMLElement => screen.getByTestId('read-all-notifications');

describe('the bell', () => {
  it('shows how many are unread, and nothing when none is', async () => {
    const { inbox } = mount([told('a', 2), told('b', 1, 5)]);
    expect(screen.queryByTestId('unread-notifications')).toBeNull();
    await inbox.refresh();
    expect(await screen.findByTestId('unread-notifications')).toHaveTextContent('1');
    inbox.unread = 0;
    await waitFor(() => {
      expect(screen.queryByTestId('unread-notifications')).toBeNull();
    });
  });

  it('lets all be marked as read only once the list is read and while one of it is unread', async () => {
    const { notifications, answer } = mount([told('a', 30), told('b', 20, 25), told('c', 10)]);
    await fireEvent.click(screen.getByTestId('open-notifications'));
    expect(await screen.findByTestId('notifications')).toBeInTheDocument();
    expect(readAll()).toBeDisabled();

    answer();
    await waitFor(() => {
      expect(readAll()).toBeEnabled();
    });
    await fireEvent.click(readAll());
    expect(notifications.readAll).toHaveBeenCalledWith(30);
    await waitFor(() => {
      expect(readAll()).toBeDisabled();
    });
  });

  it('lets nothing be marked of a list that is all read, or empty', async () => {
    for (const items of [[told('a', 2, 3)], []]) {
      const { answer } = mount(items);
      answer();
      await fireEvent.click(screen.getByTestId('open-notifications'));
      await waitFor(() => {
        expect(
          screen.queryAllByRole('listitem').length +
            screen.queryAllByTestId('no-notifications').length,
        ).toBeGreaterThan(0);
      });
      expect(readAll()).toBeDisabled();
      cleanup();
    }
  });
});
