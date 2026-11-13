<!-- One row per group; direct memberships remain in the people list. -->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardStore, Workspace } from '$lib/state/index.ts';
  import type { BoardTeam, Team, TeamRole, Teams } from '$lib/state/team-types.ts';
  import { Button } from '$lib/ui/button/index.js';

  let { workspace, board, teams }: { workspace: Workspace; board: BoardStore; teams: Teams } =
    $props();
  const id = $props.id();
  const place = $derived(workspace.info?.id);
  let assigned = $state.raw<BoardTeam[]>([]);
  let groups = $state.raw<Team[]>([]);
  let selected = $state('');
  let role = $state<TeamRole>('editor');
  let busy = $state(false);
  let problem = $state('');
  let reading = 0;
  const candidates = $derived(
    groups.filter((team) => !assigned.some((item) => item.team.id === team.id)),
  );
  const roles: TeamRole[] = ['editor', 'commenter', 'viewer'];
  const labels = {
    editor: () => m['BoardMember.schemeEditor'](),
    commenter: () => m['BoardMember.schemeCommenter'](),
    viewer: () => m['BoardMember.schemeViewer'](),
  };

  async function read(workspaceId: string, boardId: string): Promise<void> {
    const current = ++reading;
    try {
      const [all, onBoard] = await Promise.all([
        teams.list(workspaceId),
        teams.onBoard(workspaceId, boardId),
      ]);
      if (current !== reading) return;
      groups = all;
      assigned = onBoard;
    } catch {
      if (current === reading) problem = m.teams_load_failed();
    }
  }

  $effect(() => {
    // Member messages also refresh this external view after another person's changes.
    const request = { place, boardId: board.boardId, members: board.members };
    if (request.place) void read(request.place, request.boardId);
    return () => {
      reading += 1;
    };
  });

  async function change(teamId: string, next: TeamRole | undefined): Promise<void> {
    if (!place || busy) return;
    busy = true;
    problem = '';
    try {
      if (next) await teams.assign(place, board.boardId, teamId, next);
      else await teams.unassign(place, board.boardId, teamId);
      selected = '';
    } catch {
      problem = m.teams_failed();
    } finally {
      await read(place, board.boardId);
      busy = false;
    }
  }
</script>

<section class="grid gap-2" aria-labelledby="{id}-title" data-testid="board-teams">
  <h3 id="{id}-title" class="font-medium">{m.teams_title()}</h3>
  <p class="text-sm text-muted-foreground">{m.teams_board_description()}</p>
  {#if problem}<p role="alert" class="text-sm text-destructive">{problem}</p>{/if}
  <ul class="grid gap-2">
    {#each assigned as item (item.team.id)}
      <li class="rounded-md border p-2" data-board-team={item.team.name}>
        <details>
          <summary class="cursor-pointer font-medium break-words"
            >{item.team.name} ({item.team.members.length})</summary
          >
          <ul class="mt-2 grid gap-1 pl-3 text-sm">
            {#each item.team.members as person (person.userId)}
              <li>{person.displayName || person.username}</li>
            {/each}
          </ul>
        </details>
        <div class="mt-2 flex flex-wrap gap-2">
          <select
            class="h-9 rounded-md border bg-background px-2"
            aria-label={m.share_role_of({ name: item.team.name })}
            value={item.role}
            disabled={busy}
            onchange={(event) => void change(item.team.id, event.currentTarget.value as TeamRole)}
          >
            {#each roles as option (option)}<option value={option}>{labels[option]()}</option
              >{/each}
          </select>
          <Button
            variant="ghost"
            size="sm"
            disabled={busy}
            aria-label={m.teams_board_remove({ name: item.team.name })}
            onclick={() => void change(item.team.id, undefined)}
            >{m.share_remove({ name: item.team.name })}</Button
          >
        </div>
      </li>
    {/each}
  </ul>
  {#if candidates.length > 0}
    <form
      class="flex flex-wrap gap-2"
      onsubmit={(event) => {
        event.preventDefault();
        if (selected) void change(selected, role);
      }}
    >
      <select
        class="h-9 min-w-0 flex-1 rounded-md border bg-background px-2"
        aria-label={m.teams_board_choose()}
        bind:value={selected}
        disabled={busy}
      >
        <option value="">{m.teams_board_choose()}</option>
        {#each candidates as team (team.id)}<option value={team.id}>{team.name}</option>{/each}
      </select>
      <select
        class="h-9 rounded-md border bg-background px-2"
        aria-label={m.share_minimum_role()}
        bind:value={role}
        disabled={busy}
      >
        {#each roles as option (option)}<option value={option}>{labels[option]()}</option>{/each}
      </select>
      <Button type="submit" size="sm" disabled={busy || !selected}>{m.teams_board_add()}</Button>
    </form>
  {/if}
</section>
