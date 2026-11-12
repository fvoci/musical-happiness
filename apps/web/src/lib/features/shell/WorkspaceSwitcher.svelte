<!--
  The head of the sidebar when the reader works in more than one workspace:
  the name of the one shown, which opens the list of the others to go to.
  With one workspace the head is the app's name, as it was.
-->
<script lang="ts">
  import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';

  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { WorkspaceInfo } from '$lib/state/index.ts';

  interface Props {
    workspaces: readonly WorkspaceInfo[];
    /** The workspace shown. */
    current: string | undefined;
    onSwitch: (workspaceId: string) => void;
  }

  let { workspaces, current, onSwitch }: Props = $props();

  const titleOf = (workspace: WorkspaceInfo | undefined): string =>
    workspace?.title || m.org_untitled_workspace();

  const actions = $derived<MenuAction[]>(
    workspaces.map((workspace) => ({
      id: workspace.id,
      label: titleOf(workspace),
      checked: workspace.id === current,
      run: () => {
        if (workspace.id !== current) onSwitch(workspace.id);
      },
    })),
  );
</script>

<ActionMenu title={m.shell_switch_workspace()} {actions} align="start">
  {#snippet trigger(props: Record<string, unknown>)}
    <button
      {...props}
      type="button"
      data-testid="workspace-switcher"
      aria-label={m.shell_switch_workspace_from({
        name: titleOf(workspaces.find((workspace) => workspace.id === current)),
      })}
      class="flex min-h-8 min-w-0 flex-1 items-center gap-1 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
    >
      <span class="min-w-0 flex-1 truncate text-lg font-semibold tracking-tight">
        {titleOf(workspaces.find((workspace) => workspace.id === current))}
      </span>
      <ChevronsUpDownIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    </button>
  {/snippet}
</ActionMenu>
