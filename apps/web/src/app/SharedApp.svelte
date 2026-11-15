<!--
  A board read by its public link (`/shared/<board>/<view>/<card>?r=<token>`):
  its views, its cards with their bodies, comments and files, and nothing
  else; or a wiki, its tree and its pages (`SharedWiki`), the kind the server
  gives the link's board saying which. No sidebar, no settings, nobody named, nothing to change: the backend
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
  import type { BoardKind } from '$lib/logic/wire.ts';
  import { Workspace, type Backend } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { startShared } from './backend.ts';
  import BoardPage from './BoardPage.svelte';
  import { currentLocale } from './locale.ts';
  import NotFound from './NotFound.svelte';
  import Problems from './Problems.svelte';
  import type { Router } from './router.svelte.ts';
  import SharedWiki from './SharedWiki.svelte';

  let { router }: { router: Router } = $props();

  let workspace = $state.raw<Workspace | undefined>(undefined);
  let backend = $state.raw<Backend | undefined>(undefined);
  /** What the link opens, once read: a board's screen or a wiki's. */
  let kind = $state<BoardKind | undefined>(undefined);
  /** The title at the head of the page: the board's, as its screen reads it, or the wiki's. */
  let wiki = $state.raw<{ title: string; icon: string } | undefined>(undefined);
  let place = '';
  /** The screen could not start (its code did not load): nothing is read. */
  let unstarted = $state(false);

  /** The board read, once it is: its title heads the page. */
  const board = $derived(kind === 'wiki' ? wiki : workspace?.board?.board);

  onMount(() => {
    const { boardId, readToken } = router.route;
    if (!boardId || !readToken) return;
    let made: Workspace | undefined;
    let left = false;
    /** Asked as a function: `left` changes while a reading waits, which TypeScript does not see. */
    const isLeft = (): boolean => left;
    void startShared(boardId, readToken).then(
      async (started) => {
        // Gone before the backend came: nothing is read.
        if (left) return;
        const read = started.backend;
        place = started.place;
        made = new Workspace({ backend: read, locale: currentLocale });
        // The bodies name their images by file: the link's board or wiki has them.
        setFileUrlResolver((fileId) => read.fileUrl(boardId, fileId));
        workspace = made;
        backend = read;
        await made.load(place);
        // Which screen: a link that opens nothing shows the board's "not found".
        const opened = await read.board(boardId).catch(() => undefined);
        if (isLeft()) return;
        kind = opened?.kind ?? 'board';
        if (opened?.kind === 'wiki') wiki = { title: opened.title, icon: opened.icon ?? '' };
      },
      () => {
        if (!left) unstarted = true;
      },
    );
    return () => {
      left = true;
      made?.close();
    };
  });
</script>

<!-- The page is what is inside the edges the device keeps (#app has them as padding). -->
<div
  class="flex h-[calc(100dvh-var(--safe-top))] min-h-0 flex-col overflow-hidden bg-background"
  data-testid="shared-board"
>
  <header class="flex h-12 shrink-0 items-center gap-3 border-b px-4">
    <p class="shrink-0 font-semibold tracking-tight" translate="no">{m.app_name()}</p>
    {#if board}
      <h1 class="min-w-0 truncate text-muted-foreground" data-testid="shared-title">
        {board.icon ? `${board.icon} ` : ''}{board.title ||
          (kind === 'wiki' ? m.wiki_untitled() : m['Sidebar.untitled-board']())}
      </h1>
    {/if}
  </header>
  <main class="@container/pane flex min-h-0 min-w-0 flex-1 flex-col" {@attach pane.attach}>
    {#if !router.route.boardId || !router.route.readToken}
      <NotFound {router} />
    {:else if unstarted}
      <div class="grid h-full place-items-center p-6 text-center">
        <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
      </div>
    {:else if !workspace || !kind || workspace.status === 'idle' || workspace.status === 'loading'}
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
    {:else if kind === 'wiki' && backend}
      <SharedWiki {backend} wikiId={router.route.boardId} {router} />
    {:else}
      <BoardPage {workspace} {router} />
    {/if}
  </main>
</div>
{#if workspace}
  <Problems {workspace} />
{/if}
