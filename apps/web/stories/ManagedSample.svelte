<script lang="ts">
  import { onMount } from 'svelte';
  import ManagedBoard from '../src/app/ManagedBoard.svelte';
  import { Router } from '../src/app/router.svelte.ts';
  import { BackendError, type BoardRole } from '$lib/state/index.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';

  import { exampleWorkspace, loadWorkspace } from './workspace.ts';

  let { failJoin = false }: { failJoin?: boolean } = $props();
  const example = exampleWorkspace({ managed: true });
  const router = new Router('');
  let joined = $state(false);
  onMount(() => {
    void loadWorkspace(example);
    return () => {
      example.workspace.close();
    };
  });
  const addMember = example.backend.addMember.bind(example.backend);
  example.backend.addMember = (boardId: string, userId: string, role: BoardRole) =>
    failJoin
      ? Promise.reject(new BackendError(503, { detail: 'Example refusal' }))
      : addMember(boardId, userId, role);
</script>

<div class="h-[32rem]" data-testid="managed-example">
  {#if joined}
    <output data-testid="joined-board">{example.workspace.board?.role}</output>
  {:else if example.workspace.board?.status === 'failed'}
    <ManagedBoard
      workspace={example.workspace}
      board={example.workspace.board}
      {router}
      onJoined={() => {
        joined = true;
      }}
    />
  {:else}
    <p role="status">{m.app_loading()}</p>
  {/if}
  <output data-testid="managed-problems">{example.workspace.problems.length}</output>
</div>
