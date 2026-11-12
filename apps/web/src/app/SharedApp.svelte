<!--
  A board read by its public link (`/shared/<board>/<view>/<card>?r=<token>`):
  its views, its cards with their bodies, comments and files, and nothing
  else. No sidebar, no settings, nobody named, nothing to change: the backend
  of the link writes nothing (`startShared`). Whoever opens the link reads it
  so, signed in or not; no session goes along. Nothing changes while it is
  open: reading again shows what is new. A wrong token, a link turned off and
  a board in the trash all show the same "not found".
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import { setFileUrlResolver } from '$lib/editor/files.ts';
  import { pane } from '$lib/layout/room.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { startShared } from './backend.ts';
  import BoardPage from './BoardPage.svelte';
  import { currentLocale } from './locale.ts';
  import NotFound from './NotFound.svelte';
  import Problems from './Problems.svelte';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();

  let workspace = $state.raw<Workspace | undefined>(undefined);
  let place = '';

  onMount(() => {
    const { boardId, readToken } = router.route;
    if (!boardId || !readToken) return;
    let made: Workspace | undefined;
    void startShared(boardId, readToken).then(async (started) => {
      const { backend } = started;
      place = started.place;
      made = new Workspace({ backend, locale: currentLocale });
      // The bodies of cards name their images by file: the link's board has them.
      setFileUrlResolver((fileId) => backend.fileUrl(boardId, fileId));
      workspace = made;
      await made.load(place);
    });
    return () => {
      made?.close();
    };
  });
</script>

<div class="flex h-dvh min-h-0 flex-col bg-background" data-testid="shared-board">
  <header class="flex h-12 shrink-0 items-center border-b px-4">
    <p class="font-semibold tracking-tight" translate="no">{m.app_name()}</p>
  </header>
  <main class="@container/pane flex min-h-0 min-w-0 flex-1 flex-col" {@attach pane.attach}>
    {#if !router.route.boardId}
      <NotFound {router} />
    {:else if !workspace || workspace.status === 'idle' || workspace.status === 'loading'}
      <div class="grid h-full place-items-center p-6">
        <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
      </div>
    {:else if workspace.status !== 'ready'}
      <div class="grid h-full place-items-center p-6 text-center">
        <div>
          <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
          <Button class="mt-4 roomy:min-h-touch" onclick={() => void workspace?.load(place)}>
            {m.app_retry()}
          </Button>
        </div>
      </div>
    {:else}
      <BoardPage {workspace} {router} />
    {/if}
  </main>
</div>
{#if workspace}
  <Problems {workspace} />
{/if}
