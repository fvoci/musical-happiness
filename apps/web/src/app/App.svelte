<!--
  The app: starts the backend, loads the reader's workspace, and puts the
  screens together. Features do not import each other; this folder is where
  they meet (src/lib/README.md).
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import { setFileUrlResolver } from '$lib/editor/files.ts';
  import AppShell from '$lib/features/shell/AppShell.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { startBackend } from './backend.ts';
  import BoardPage from './BoardPage.svelte';
  import { currentLocale } from './locale.ts';
  import Problems from './Problems.svelte';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();

  let workspace = $state.raw<Workspace | undefined>(undefined);

  onMount(() => {
    void startBackend().then(async ({ backend }) => {
      const made = new Workspace({ backend, locale: currentLocale });
      // The bodies of cards name their images by file; the board that is open says where they are.
      setFileUrlResolver((fileId) =>
        made.board ? backend.fileUrl(made.board.boardId, fileId) : undefined,
      );
      workspace = made;
      await made.load();
    });
    return () => {
      workspace?.close();
    };
  });

  // Without a board in the address, show the first one of the sidebar.
  $effect(() => {
    if (workspace?.status !== 'ready' || router.route.unknown || router.route.boardId) return;
    const first = workspace.sidebar.flatMap((entry) => entry.boards)[0];
    if (first) router.go({ boardId: first.id }, { replace: true });
  });
</script>

{#if !workspace || workspace.status === 'idle' || workspace.status === 'loading'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
      <p class="mt-2 text-muted-foreground" role="status">{m.app_loading()}</p>
    </div>
  </main>
{:else if workspace.status === 'failed'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-lg font-medium" role="alert">{m.app_load_failed()}</p>
      <Button class="mt-4 roomy:min-h-touch" onclick={() => void workspace?.load()}>
        {m.app_retry()}
      </Button>
    </div>
  </main>
{:else}
  <AppShell
    {workspace}
    boardId={router.route.boardId}
    boardHref={(boardId: string) => router.href({ boardId })}
    onOpenBoard={(boardId: string) => {
      router.openBoard(boardId);
    }}
  >
    {#if router.route.unknown}
      <div class="grid h-full place-items-center p-6 text-center">
        <p class="text-muted-foreground" role="alert">{m.app_page_not_found()}</p>
      </div>
    {:else if router.route.boardId}
      <BoardPage {workspace} {router} />
    {:else}
      <div class="grid h-full place-items-center p-6 text-center">
        <p class="text-muted-foreground">{m.app_no_boards()}</p>
      </div>
    {/if}
  </AppShell>
  <Problems {workspace} />
{/if}
