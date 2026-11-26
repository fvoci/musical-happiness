<!--
  Dialog to copy a board, wiki, or course into another session-opened workspace
  (`POST /w/{ws}/containers/{containerId}/copy-to-workspace`).
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import { nameOfWorkspace } from '$lib/components/workspace-name.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Container } from '$lib/logic/wire.ts';
  import { isBackendError, isOpen, isOwnSpace, type Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';
  import { Input } from '$lib/ui/input/index.js';

  import { shownTitle } from './shown-title.ts';

  interface Props {
    open: boolean;
    workspace: Workspace;
    container: Container | undefined;
    onCopied: (copied: Container) => void;
  }

  let { open = $bindable(), workspace, container, onCopied }: Props = $props();

  const uid = $props.id();
  let targetWorkspaceId = $state('');
  let customTitle = $state('');
  let busy = $state(false);
  let errorMessage = $state<string | undefined>(undefined);

  const targets = $derived(
    workspace.workspaces.filter(isOpen).filter((ws) => ws.id !== container?.workspaceId),
  );

  function labelOf(ws: (typeof targets)[number]): string {
    return isOwnSpace(ws)
      ? m.own_space_name()
      : nameOfWorkspace(ws, m.shell_workspace_unnamed_title);
  }

  function opened(): void {
    untrack(() => {
      targetWorkspaceId = targets[0]?.id ?? '';
      customTitle = container?.title ?? '';
      busy = false;
      errorMessage = undefined;
    });
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!container || !targetWorkspaceId || busy) return;
    busy = true;
    errorMessage = undefined;
    try {
      const trimmed = customTitle.trim();
      const out = await workspace.copyContainerToWorkspace(container.id, {
        targetWorkspaceId,
        title: trimmed ? trimmed : null,
      });
      open = false;
      onCopied(out.container);
    } catch (error) {
      if (isBackendError(error) && error.reason === 'copy_out_disabled') {
        errorMessage = m.shell_copy_to_workspace_disabled();
      } else if (
        isBackendError(error) &&
        (error.status === 413 || error.reason === 'storage_full')
      ) {
        errorMessage = m.shell_copy_to_workspace_storage_full();
      } else {
        errorMessage = m.shell_copy_to_workspace_failed();
      }
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content data-testid="copy-to-workspace-dialog" {@attach opened}>
    <form class="grid gap-4" onsubmit={(event) => void submit(event)}>
      <Dialog.Header>
        <Dialog.Title>{m.shell_copy_to_workspace_title()}</Dialog.Title>
        <Dialog.Description>
          {m.shell_copy_to_workspace_description({
            title: container ? shownTitle(container) : '',
          })}
        </Dialog.Description>
      </Dialog.Header>

      <div class="grid gap-1.5">
        <label for="{uid}-target" class="text-sm font-medium">
          {m.shell_copy_to_workspace_target()}
        </label>
        <select
          id="{uid}-target"
          data-testid="copy-to-workspace-target"
          class="h-9 rounded-md border border-input-border bg-background px-3 text-sm roomy:h-touch roomy:text-base"
          bind:value={targetWorkspaceId}
        >
          {#each targets as ws (ws.id)}
            <option value={ws.id}>{labelOf(ws)}</option>
          {/each}
        </select>
      </div>

      <Input
        bind:value={customTitle}
        maxlength={255}
        autocomplete="off"
        data-testid="copy-to-workspace-title"
        aria-label={m.diary_name()}
        placeholder={container ? shownTitle(container) : ''}
        class="roomy:h-touch roomy:text-base"
      />

      {#if errorMessage}
        <p class="text-sm text-destructive" role="alert" data-testid="copy-to-workspace-error">
          {errorMessage}
        </p>
      {/if}

      <Dialog.Footer>
        <Button
          variant="outline"
          class="roomy:min-h-touch"
          disabled={busy}
          onclick={() => {
            open = false;
          }}
        >
          {m.shared_cancel()}
        </Button>
        <Button
          type="submit"
          class="roomy:min-h-touch"
          disabled={busy || !targetWorkspaceId}
          aria-busy={busy}
          data-testid="copy-to-workspace-submit"
        >
          {m.shell_copy_to_workspace_submit()}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
