<!--
  Copying a board, a wiki or a course into another workspace the session opens
  (`POST /w/{ws}/containers/{containerId}/copy-to-workspace`), with one press:
  the copy keeps the title and is private, so there is nothing to name or
  choose but where it goes, and that is chosen already where there is one
  place to go. The copy opens once it is made, its title there to be changed.

  An organization that keeps its containers in (`allowCopyOut` off) lets them
  be copied only into its own workspaces. Where that is known (to an
  administrator who read its settings, or from a copy it refused), the places
  outside it cannot be chosen, and the dialog says why.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import { nameOfWorkspace } from '$lib/components/workspace-name.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Container } from '$lib/logic/wire.ts';
  import {
    copyOutPlace,
    isBackendError,
    isOpen,
    isOwnSpace,
    keepsCopiesIn,
    noteCopyOut,
    type Workspace,
    type WorkspaceInfo,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';

  import { shownTitle } from './shown-title.ts';

  interface Props {
    open: boolean;
    workspace: Workspace;
    container: Container | undefined;
    onCopied: (copied: Container) => void;
  }

  let { open = $bindable(), workspace, container, onCopied }: Props = $props();

  const uid = $props.id();
  let chosenId = $state('');
  let busy = $state(false);
  let errorMessage = $state<string | undefined>(undefined);

  const source = $derived(
    workspace.workspaces.find((ws) => ws.id === container?.workspaceId) ?? workspace.info,
  );
  const places = $derived(
    workspace.workspaces.filter(isOpen).filter((ws) => ws.id !== container?.workspaceId),
  );
  /** The source keeps its containers in, as far as is known: only its own workspaces take a copy. */
  const keptIn = $derived(source !== undefined && keepsCopiesIn(copyOutPlace(source)));
  const outside = (ws: WorkspaceInfo): boolean =>
    source !== undefined && copyOutPlace(ws) !== copyOutPlace(source);
  const allowed = $derived(places.filter((ws) => !keptIn || !outside(ws)));
  /** The place chosen, or the first allowed while the one chosen is not (any longer). */
  const targetId = $derived(
    allowed.some((ws) => ws.id === chosenId) ? chosenId : (allowed[0]?.id ?? ''),
  );

  function labelOf(ws: WorkspaceInfo): string {
    const name = isOwnSpace(ws)
      ? m.own_space_name()
      : nameOfWorkspace(ws, m.shell_workspace_unnamed_title);
    return keptIn && outside(ws) ? m.shell_copy_to_workspace_kept_option({ name }) : name;
  }

  function opened(): void {
    untrack(() => {
      chosenId = '';
      busy = false;
      errorMessage = undefined;
    });
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!container || !targetId || busy) return;
    busy = true;
    errorMessage = undefined;
    try {
      const out = await workspace.copyContainerToWorkspace(container.id, {
        targetWorkspaceId: targetId,
        title: container.title.trim() === '' ? null : container.title,
      });
      open = false;
      onCopied(out.container);
    } catch (error) {
      if (isBackendError(error) && error.reason === 'copy_out_disabled') {
        if (source) noteCopyOut(copyOutPlace(source), false);
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

      {#if places.length === 1 && allowed[0]}
        {@const only = allowed[0]}
        <p class="text-sm" data-testid="copy-to-workspace-only">
          {m.shell_copy_to_workspace_only({ name: labelOf(only) })}
        </p>
      {:else}
        <div class="grid gap-1.5">
          <label for="{uid}-target" class="text-sm font-medium">
            {m.shell_copy_to_workspace_target()}
          </label>
          <select
            id="{uid}-target"
            data-testid="copy-to-workspace-target"
            class="h-9 rounded-md border border-input-border bg-background px-3 text-base md:text-sm roomy:h-touch"
            aria-describedby={keptIn ? `${uid}-kept` : undefined}
            bind:value={() => targetId, (next: string) => (chosenId = next)}
          >
            {#each places as ws (ws.id)}
              <option value={ws.id} disabled={keptIn && outside(ws)}>{labelOf(ws)}</option>
            {/each}
          </select>
        </div>
      {/if}

      {#if keptIn && allowed.length < places.length}
        <p
          id="{uid}-kept"
          class="text-sm text-muted-foreground"
          data-testid="copy-to-workspace-kept"
        >
          {m.shell_copy_to_workspace_kept()}
        </p>
      {/if}

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
          disabled={busy || !targetId}
          aria-busy={busy}
          data-testid="copy-to-workspace-submit"
        >
          {m.shell_copy_to_workspace_submit()}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
