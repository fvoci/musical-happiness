<!--
  Archives on their way: one line each at the foot of the page, with how much
  has arrived or gone, until a moment after they are done. A failure is told
  where problems are (the workspace reports it), not here.
-->
<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { humanFileSize } from '$lib/logic/format.ts';
  import type { Transfer, Workspace } from '$lib/state/index.ts';

  let { workspace }: { workspace: Workspace } = $props();

  function label(transfer: Transfer): string {
    const name = transfer.name || m['Sidebar.untitled-board']();
    if (transfer.finished) {
      return transfer.kind === 'export'
        ? m.shell_transfer_done_export({ name })
        : m.shell_transfer_done_import({ name });
    }
    return transfer.kind === 'export'
      ? m.shell_transfer_export({ name })
      : m.shell_transfer_import({ name });
  }
</script>

<!-- In the page from the start: a live region that appears with its words is often not read. -->
<div
  class="pointer-events-none fixed bottom-[calc(1rem+var(--safe-bottom))] left-[calc(1rem+var(--safe-left))] z-50 grid w-[min(22rem,calc(100%-2rem))] gap-2"
  role="status"
  data-testid="transfers"
>
  {#each workspace.transfers as transfer (transfer.id)}
    <div
      class="pointer-events-auto grid gap-1.5 rounded-md border bg-popover p-3 text-sm text-popover-foreground shadow-md"
      data-transfer={transfer.kind}
      data-finished={transfer.finished || undefined}
    >
      <div class="flex items-start gap-2">
        <p class="min-w-0 flex-1">{label(transfer)}</p>
        <IconButton
          label={m.app_problem_dismiss()}
          class="-my-1 -mr-1 size-6"
          onclick={() => {
            workspace.dismissTransfer(transfer.id);
          }}
        >
          <XIcon />
        </IconButton>
      </div>
      {#if !transfer.finished}
        <progress
          class="h-1.5 w-full accent-primary"
          max={transfer.total || undefined}
          value={transfer.total ? transfer.done : undefined}
          aria-label={label(transfer)}
        ></progress>
        {#if transfer.done > 0}
          <p class="text-xs text-muted-foreground">
            {transfer.total
              ? m.shell_transfer_progress({
                  done: humanFileSize(transfer.done),
                  total: humanFileSize(transfer.total),
                })
              : humanFileSize(transfer.done)}
          </p>
        {/if}
      {/if}
    </div>
  {/each}
</div>
