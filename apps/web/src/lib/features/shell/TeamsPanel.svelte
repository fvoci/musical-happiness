<!-- Groups in this workspace. Members read them; administrators manage them. -->
<script lang="ts">
  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { administers } from '$lib/state/people.ts';
  import {
    isBackendError,
    type People,
    type Workspace,
    type WorkspaceMember,
  } from '$lib/state/index.ts';
  import type { Team, Teams } from '$lib/state/team-types.ts';
  import { Button } from '$lib/ui/button/index.js';
  import TeamRow from './TeamRow.svelte';

  let {
    workspace,
    people,
    teams,
    membersChanged = 0,
  }: { workspace: Workspace; people: People; teams: Teams; membersChanged?: number } = $props();
  const id = $props.id();
  const workspaceId = $derived(workspace.info?.id);
  const managing = $derived(administers(workspace));
  const workspaceName = $derived(workspace.info?.title || m.shell_workspace_unnamed());
  let groups = $state.raw<Team[]>([]);
  let members = $state.raw<WorkspaceMember[]>([]);
  let loaded = $state(false);
  let working = $state(false);
  let problem = $state('');
  let name = $state('');
  let deleting = $state.raw<Team | undefined>(undefined);
  let askOpen = $state(false);
  let reading = 0;

  async function read(place: string, manager: boolean): Promise<void> {
    const current = ++reading;
    try {
      const [listed, candidates] = await Promise.all([
        teams.list(place),
        manager ? people.members(place) : Promise.resolve([]),
      ]);
      if (current !== reading) return;
      groups = listed;
      members = candidates.filter((member) => member.role !== 'guest');
      loaded = true;
    } catch {
      if (current === reading) problem = m.teams_load_failed();
    }
  }

  // External I/O follows the workspace and permissions, including an open dialog.
  $effect(() => {
    if (workspaceId && membersChanged >= 0) void read(workspaceId, managing);
    return () => {
      reading += 1;
    };
  });

  async function act(step: (place: string) => Promise<unknown>): Promise<void> {
    if (!workspaceId || working) return;
    working = true;
    problem = '';
    try {
      await step(workspaceId);
    } catch (error) {
      problem =
        isBackendError(error) && error.status === 409 ? m.teams_name_taken() : m.teams_failed();
    } finally {
      await read(workspaceId, managing);
      working = false;
    }
  }
</script>

<section class="grid gap-3" aria-labelledby="{id}-title" data-testid="teams-panel">
  <div>
    <h3 id="{id}-title" class="font-medium">{m.teams_title()}</h3>
    <p class="text-sm text-muted-foreground">{m.teams_description({ workspace: workspaceName })}</p>
  </div>
  {#if problem}<p role="alert" class="text-sm text-destructive">{problem}</p>{/if}
  {#if managing}
    <form
      class="flex flex-wrap gap-2"
      onsubmit={(event) => {
        event.preventDefault();
        if (name.trim())
          void act(async (place) => {
            await teams.create(place, name);
            name = '';
          });
      }}
    >
      <input
        class="h-9 min-w-0 flex-1 rounded-md border border-input-border bg-background px-2"
        aria-label={m.teams_name()}
        placeholder={m.teams_name()}
        bind:value={name}
        disabled={working}
      />
      <Button type="submit" size="sm" disabled={working || !name.trim()}>{m.teams_create()}</Button>
    </form>
  {/if}
  {#if loaded && groups.length === 0}<p class="text-sm text-muted-foreground">
      {m.teams_empty()}
    </p>{/if}
  <ul class="grid gap-2">
    {#each groups as team (team.id)}
      <TeamRow
        {team}
        {members}
        {managing}
        {working}
        onRename={(value: string) => act((place) => teams.rename(place, team.id, value))}
        onAdd={(userId: string) => act((place) => teams.addMember(place, team.id, userId))}
        onRemove={(userId: string) => act((place) => teams.removeMember(place, team.id, userId))}
        onDelete={() => {
          deleting = team;
          askOpen = true;
        }}
      />
    {/each}
  </ul>
</section>

<ConfirmDialog
  bind:open={askOpen}
  title={m.teams_delete_title({ name: deleting?.name ?? '' })}
  description={m.teams_delete_description()}
  confirmLabel={m.teams_delete()}
  destructive
  onConfirm={() => {
    if (deleting) {
      const team = deleting;
      void act((place) => teams.delete(place, team.id));
    }
  }}
/>
