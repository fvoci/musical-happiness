<!--
  The head of the sidebar: the name of the workspace shown, which opens the
  list of the reader's workspaces to go to. The reader's own space comes first,
  as "My space" with a mark of its own, then the others in the backend's order;
  where the reader may start an organization (`onCreateOrganization`), the list
  ends with that.
-->
<script lang="ts">
  import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import UserRoundIcon from '@lucide/svelte/icons/user-round';
  import UsersRoundIcon from '@lucide/svelte/icons/users-round';

  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { nameOfWorkspace } from '$lib/components/workspace-name.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isOwnSpace, type WorkspaceInfo } from '$lib/state/index.ts';

  import NewOrganizationDialog from './NewOrganizationDialog.svelte';

  interface Props {
    workspaces: readonly WorkspaceInfo[];
    /** The workspace shown. */
    current: string | undefined;
    onSwitch: (workspaceId: string) => void;
    /** Starts an organization of that name and goes to it; a refusal rejects. Left out where none can be started. */
    onCreateOrganization?: ((name: string) => Promise<void>) | undefined;
  }

  let { workspaces, current, onSwitch, onCreateOrganization }: Props = $props();

  let creating = $state(false);

  const titleOf = (workspace: WorkspaceInfo | undefined): string =>
    nameOfWorkspace(workspace, m.org_untitled_workspace);

  const shown = $derived(workspaces.find((workspace) => workspace.id === current));
  /**
   * The own space first, the others as the backend lists them. To come with the
   * server's refusal of a provider's session there: the own space of an account
   * that has a password opens only to a session its password made, and is shown so.
   */
  const ordered = $derived([
    ...workspaces.filter(isOwnSpace),
    ...workspaces.filter((workspace) => !isOwnSpace(workspace)),
  ]);

  const actions = $derived<MenuAction[]>([
    ...ordered.map((workspace) => ({
      id: workspace.id,
      label: titleOf(workspace),
      icon: isOwnSpace(workspace) ? UserRoundIcon : UsersRoundIcon,
      checked: workspace.id === current,
      ...(workspace.id === current
        ? { ariaLabel: m.shell_workspace_current({ name: titleOf(workspace) }) }
        : {}),
      run: () => {
        if (workspace.id !== current) onSwitch(workspace.id);
      },
    })),
    ...(onCreateOrganization
      ? [
          {
            id: 'new-organization',
            label: m.org_new(),
            icon: PlusIcon,
            separated: workspaces.length > 0,
            run: () => {
              creating = true;
            },
          },
        ]
      : []),
  ]);
</script>

<ActionMenu title={m.shell_switch_workspace()} {actions} align="start">
  {#snippet trigger(props: Record<string, unknown>)}
    <button
      {...props}
      type="button"
      data-testid="workspace-switcher"
      data-own-space={isOwnSpace(shown) ? '' : undefined}
      aria-label={m.shell_switch_workspace_from({ name: titleOf(shown) })}
      class="flex min-h-8 min-w-0 flex-1 items-center gap-1.5 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
    >
      {#if isOwnSpace(shown)}
        <UserRoundIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      {/if}
      <span class="min-w-0 flex-1 truncate text-lg font-semibold tracking-tight">
        {titleOf(shown)}
      </span>
      <ChevronsUpDownIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    </button>
  {/snippet}
</ActionMenu>

{#if onCreateOrganization}
  <NewOrganizationDialog bind:open={creating} onCreate={onCreateOrganization} />
{/if}
