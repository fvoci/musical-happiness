<script lang="ts">
  import WorkspaceSwitcher from '$lib/features/shell/WorkspaceSwitcher.svelte';
  import { BackendError, type WorkspaceInfo } from '$lib/state/index.ts';

  let {
    count = 4,
    long = false,
    own = false,
    inOwn = false,
    starts,
  }: {
    count?: number;
    long?: boolean;
    /** The reader's own space is listed, last as the server lists it. */
    own?: boolean;
    /** The own space is the one shown. */
    inOwn?: boolean;
    /** Organizations can be started: made, or refused as the server refuses one past its limit. */
    starts?: 'made' | 'limit';
  } = $props();

  const listed = $derived<WorkspaceInfo[]>([
    ...Array.from({ length: count }, (_, index) => ({
      id: `example-workspace-${index}`,
      kind: 'team' as const,
      title:
        index === 0 && long
          ? 'Example workspace · 이름이 긴 작업 공간 '.repeat(8)
          : `Example workspace ${index} · 예시 작업 공간`,
    })),
    ...(own ? [{ id: 'own-space', kind: 'personal' as const, title: '' }] : []),
  ]);
  let started = $state<WorkspaceInfo[]>([]);
  const workspaces = $derived([...started, ...listed]);
  let current = $derived(inOwn ? 'own-space' : listed[0]?.id);

  async function createOrganization(name: string): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 50));
    if (starts === 'limit') {
      throw new BackendError(400, { reason: 'too_many_organizations', limit: 3 });
    }
    const made = { id: `started-${started.length}`, kind: 'team' as const, title: name };
    started = [...started, made];
    current = made.id;
  }
</script>

<div class="grid max-w-sm gap-4">
  <div class="flex min-w-0 bg-sidebar p-2">
    <WorkspaceSwitcher
      {workspaces}
      {current}
      onSwitch={(id: string) => {
        current = id;
      }}
      onCreateOrganization={starts ? createOrganization : undefined}
    />
  </div>
  <output data-testid="workspace-choice" class="text-xs text-muted-foreground"
    >{current ?? ''}</output
  >
</div>
