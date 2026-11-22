<!--
  The bell of the top bar: how many notifications the reader has not read,
  and, opened, the newest of them, the unread marked. One is a link to its
  card or page: a plain click goes there (`onGo`, which marks it read), and
  ⌘/Ctrl or a middle click opens it in a new tab as any link does. "Mark all
  as read" marks those shown and what came before them.

  The count and the list come from the inbox (`NotificationInbox`), which
  keeps them up to date while the page is open; the list is read when the
  bell is opened.
-->
<script lang="ts">
  import BellIcon from '@lucide/svelte/icons/bell';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { relativeTime } from '$lib/logic/format.ts';
  import { userDisplayName } from '$lib/logic/users.ts';
  import type { NotificationInbox } from '$lib/state/notification-inbox.svelte.ts';
  import type { Notification } from '$lib/state/notification-types.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Popover from '$lib/ui/popover/index.js';

  import { notificationText, unreadBadge } from './notification-words.ts';

  interface Props {
    inbox: NotificationInbox;
    /** The language times are written in. */
    locale: string;
    /** The address of a notification's card or page, for its link. */
    hrefOf: (notification: Notification) => string;
    /** Goes to a notification's card or page; the bell has closed by then. */
    onGo: (notification: Notification) => void;
  }

  let { inbox, locale, hrefOf, onGo }: Props = $props();

  let open = $state(false);
  /** When the list was last shown: the times are written from it. */
  let now = $state(Date.now());

  const label = $derived(
    inbox.unread > 0
      ? m.notifications_open_unread({ count: String(inbox.unread) })
      : m.notifications_open(),
  );

  function nameOf(notification: Notification): string | undefined {
    const actorId = notification.subject?.actorId;
    const user = actorId ? inbox.people.get(actorId) : undefined;
    return user ? userDisplayName(user) : undefined;
  }

  function toggled(next: boolean): void {
    open = next;
    if (next) {
      now = Date.now();
      void inbox.open();
    } else {
      inbox.close();
    }
  }

  function onclick(event: MouseEvent, notification: Notification): void {
    // A click with a key that opens a new tab is the browser's.
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
    event.preventDefault();
    toggled(false);
    onGo(notification);
  }
</script>

<Popover.Root bind:open={() => open, toggled}>
  <Popover.Trigger>
    {#snippet child({ props }: { props: Record<string, unknown> })}
      <IconButton {...props} {label} data-testid="open-notifications" class="relative">
        <BellIcon />
        {#if inbox.unread > 0}
          <span
            class="absolute top-0.5 right-0.5 min-w-4 rounded-full bg-primary px-1 text-[0.625rem] leading-4 font-semibold text-primary-foreground tabular-nums"
            data-testid="unread-notifications"
            aria-hidden="true">{unreadBadge(inbox.unread)}</span
          >
        {/if}
      </IconButton>
    {/snippet}
  </Popover.Trigger>
  <Popover.Content
    align="end"
    role="dialog"
    aria-label={m.notifications_title()}
    data-testid="notifications"
    class="max-h-[min(32rem,calc(100dvh-4rem))] w-[min(24rem,calc(100vw-1rem-var(--safe-left)-var(--safe-right)))] gap-0 overflow-y-auto p-0"
  >
    <div class="flex items-center gap-2 border-b px-3 py-2">
      <h2 class="flex-1 font-semibold">{m.notifications_title()}</h2>
      <Button
        variant="ghost"
        size="sm"
        disabled={!inbox.listed || !inbox.items.some((one) => one.readAt === 0)}
        data-testid="read-all-notifications"
        onclick={() => void inbox.readAll()}>{m.notifications_read_all()}</Button
      >
    </div>
    {#if !inbox.listed}
      <p class="p-4 text-muted-foreground">{m.notifications_loading()}</p>
    {:else if inbox.items.length === 0}
      <p class="p-4 text-muted-foreground" data-testid="no-notifications">
        {m.notifications_empty()}
      </p>
    {:else}
      <ul class="grid">
        {#each inbox.items as notification (notification.id)}
          {@const unread = notification.readAt === 0}
          <li data-notification={notification.id} data-unread={unread || undefined}>
            {#if notification.subject}
              <a
                href={hrefOf(notification)}
                class="flex gap-2 px-3 py-2 hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
                onclick={(event: MouseEvent) => {
                  onclick(event, notification);
                }}
              >
                {@render line(notification, unread)}
              </a>
            {:else}
              <div class="flex gap-2 px-3 py-2 text-muted-foreground">
                {@render line(notification, unread)}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </Popover.Content>
</Popover.Root>

{#snippet line(notification: Notification, unread: boolean)}
  <span
    class={['mt-1.5 size-2 shrink-0 rounded-full', unread ? 'bg-primary' : 'bg-transparent']}
    aria-hidden="true"
  ></span>
  <span class="grid min-w-0 flex-1 gap-0.5">
    <span class={[unread && 'font-medium']}
      >{notificationText(notification.subject, nameOf(notification))}</span
    >
    <span class="text-xs text-muted-foreground">
      {relativeTime(notification.updateAt, locale, Math.max(now, notification.updateAt))}
      {#if unread}<span class="sr-only">· {m.notifications_unread()}</span>{/if}
    </span>
  </span>
{/snippet}
