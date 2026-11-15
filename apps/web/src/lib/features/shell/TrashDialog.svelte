<!--
  What is in a trash, and bringing one back: the boards and wikis the reader
  was an admin of (from the sidebar; a wiki says so), the cards of the open board (from its top
  bar, for its editors), or the pages of the open wiki (from its head, for its editors: each
  says how many pages below it come back with it). Each row says what it was, who deleted it and when,
  and has its "Restore" button; one that is back leaves the list and shows
  where it was. Nothing is deleted for good here: the server empties its
  trash after a while.

  The list is read each time the dialog opens, a page at a time ("Show
  more"). A list that could not be read says so, with a button to try again.
-->
<script lang="ts">
  import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { displayDateTime, relativeTime } from '$lib/logic/format.ts';
  import type { Trash, Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';

  interface Props {
    open: boolean;
    /** Made when the dialog opens: a new reading each time. */
    trash: Trash | undefined;
    workspace: Workspace;
    title: string;
    description: string;
    /** The name of one without a title. */
    untitled: string;
    testId: string;
  }

  let {
    open = $bindable(),
    trash,
    workspace,
    title,
    description,
    untitled,
    testId,
  }: Props = $props();

  // The back button closes it, as the other dialogs: the page under it stays.
  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );

  const context = $derived({
    locale: workspace.locale,
    timeZone: workspace.regional.timeZone,
    hourCycle: workspace.regional.hourCycle,
  });

  const nameOf = (userId: string): string =>
    (userId === '' ? undefined : workspace.userName(userId)) ?? m['centerPanel.unknown-user']();
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    data-testid={testId}
    class="flex max-h-[calc(100dvh-2rem-var(--safe-top)-var(--safe-bottom))] flex-col gap-3"
  >
    <Dialog.Header>
      <Dialog.Title>{title}</Dialog.Title>
      <Dialog.Description>{description}</Dialog.Description>
    </Dialog.Header>

    {#if trash}
      <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        {#if trash.items.length > 0}
          <ul class="grid gap-1" data-testid="trash-items">
            {#each trash.items as item (item.id)}
              {@const wiki = item.kind === 'wiki'}
              {@const name = item.title || (wiki ? m.wiki_untitled() : untitled)}
              <li
                class="flex min-h-8 items-center gap-2 roomy:min-h-touch"
                data-trash-item={name}
                data-kind={item.kind}
              >
                <span class="grid min-w-0 flex-1 gap-0.5">
                  <span class="flex min-w-0 items-center gap-1.5">
                    <span class="truncate font-medium">{name}</span>
                    {#if wiki}
                      <span
                        class="shrink-0 rounded-sm border px-1 text-xs text-muted-foreground"
                        data-testid="trash-kind">{m.shell_wiki_kind()}</span
                      >
                    {/if}
                  </span>
                  <span class="truncate text-xs text-muted-foreground">
                    {#if item.below}
                      <span data-testid="trash-below"
                        >{m.shell_trash_below({ count: item.below })}</span
                      > ·
                    {/if}
                    {m.shell_trash_deleted_by({ name: nameOf(item.deletedBy) })} ·
                    <time
                      datetime={new Date(item.deletedAt).toISOString()}
                      title={displayDateTime(item.deletedAt, context)}
                    >
                      {relativeTime(item.deletedAt, workspace.locale)}
                    </time>
                  </span>
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  class="roomy:min-h-touch"
                  aria-label={m.shell_trash_restore_named({ title: name })}
                  disabled={trash.restoring.includes(item.id)}
                  onclick={() => void trash.restore(item.id)}
                >
                  <RotateCcwIcon aria-hidden="true" />
                  {m.shell_trash_restore()}
                </Button>
              </li>
            {/each}
          </ul>
        {:else if trash.status === 'ready'}
          <p class="py-4 text-center text-muted-foreground" data-testid="trash-empty">
            {m.shell_trash_empty()}
          </p>
        {/if}

        {#if trash.status === 'loading'}
          <p class="py-2 text-center text-muted-foreground" role="status">{m.app_loading()}</p>
        {:else if trash.status === 'failed'}
          <div class="grid justify-items-center gap-2 py-4 text-center">
            <p role="alert">{m.shell_trash_failed()}</p>
            <Button variant="outline" class="roomy:min-h-touch" onclick={() => void trash.retry()}>
              {m.app_retry()}
            </Button>
          </div>
        {:else if trash.hasMore}
          <div class="flex justify-center py-2">
            <Button variant="ghost" class="roomy:min-h-touch" onclick={() => void trash.more()}>
              {m.shell_trash_more()}
            </Button>
          </div>
        {/if}
      </div>
    {/if}

    <Dialog.Footer>
      <Button
        variant="outline"
        class="roomy:min-h-touch"
        onclick={() => {
          open = false;
        }}
      >
        {m.ui_close()}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
