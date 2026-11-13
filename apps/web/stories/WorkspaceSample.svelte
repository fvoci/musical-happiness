<script lang="ts">
  import WorkspaceSwitcher from '$lib/features/shell/WorkspaceSwitcher.svelte';

  let { count = 4, long = false }: { count?: number; long?: boolean } = $props();
  const workspaces = $derived(
    Array.from({ length: count }, (_, index) => ({
      id: `example-workspace-${index}`,
      title:
        index === 0 && long
          ? 'Example workspace · 이름이 긴 작업 공간 '.repeat(8)
          : `Example workspace ${index} · 예시 작업 공간`,
    })),
  );
  let current = $derived(workspaces[0]?.id);
</script>

<div class="grid max-w-sm gap-4">
  <div class="flex min-w-0 bg-sidebar p-2">
    <WorkspaceSwitcher
      {workspaces}
      {current}
      onSwitch={(id: string) => {
        current = id;
      }}
    />
  </div>
  <output data-testid="workspace-choice" class="text-xs text-muted-foreground"
    >{current ?? ''}</output
  >
</div>
