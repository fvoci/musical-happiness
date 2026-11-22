<!--
  The reader's notifications, where the features meet: the bell (in the top
  bar, beside the search), the inbox that keeps its count, and going to what
  a notification is about by the router (`places.ts`), in its own workspace,
  at its comment where it has one (`reveal-comment.ts`). Going there marks it
  read.

  Only where the backend keeps notifications: none through a public link.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import NotificationBell from '$lib/features/notifications/NotificationBell.svelte';
  import type { Workspace } from '$lib/state/index.ts';
  import { NotificationInbox } from '$lib/state/notification-inbox.svelte.ts';
  import type { Notification } from '$lib/state/notification-types.ts';

  import { focusThePlace, goToPlace, routeOfPlace, type Place } from './places.ts';
  import { revealComment } from './reveal-comment.ts';
  import type { Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

  /** One for the backend, which is the page's for its life. */
  const inbox = $derived.by(() => {
    const { notifications } = workspace.backend;
    if (!notifications) return undefined;
    return new NotificationInbox(notifications, workspace.backend, (error: unknown) => {
      workspace.report('load', error);
    });
  });

  // Listens while the bell is there.
  $effect(() => inbox?.start());

  /** Waiting for the comment of the last notification gone to, if it is still on its way. */
  let revealing: (() => void) | undefined;

  function placeOf({ subject }: Notification): Place | undefined {
    if (!subject) return undefined;
    const { blockKind: kind, blockId: id, containerId, workspaceId } = subject;
    return { kind, id, containerId, workspaceId };
  }

  function hrefOf(notification: Notification): string {
    const place = placeOf(notification);
    return place ? router.href(routeOfPlace(router, place)) : '';
  }

  /** Marks it read and goes to it, once the bell has closed; then to its comment. */
  async function go(notification: Notification): Promise<void> {
    const place = placeOf(notification);
    if (!place || !inbox) return;
    void inbox.read(notification);
    await tick();
    goToPlace(router, place);
    revealing?.();
    const commentId = notification.subject?.commentId;
    revealing = commentId ? revealComment(commentId) : undefined;
    await tick();
    settleFocus();
  }

  /** The focus, left on nothing by the bell that closed, goes to the card shown, else to the page. */
  function settleFocus(): void {
    const at = document.activeElement;
    if (at instanceof HTMLElement && at !== document.body && at.isConnected) return;
    focusThePlace();
  }
</script>

{#if inbox}
  <NotificationBell
    {inbox}
    locale={workspace.locale}
    {hrefOf}
    onGo={(notification: Notification) => void go(notification)}
  />
{/if}
