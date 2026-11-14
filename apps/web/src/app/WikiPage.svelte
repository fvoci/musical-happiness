<!--
  One wiki: its pages as a tree and the page the address names. Which wiki
  and page comes from the address; this keeps the stores and the address in
  step.

  A wiki's address without a page shows its first page, the address put in
  place (no entry of the history). A wiki not there for the reader, a board's
  identifier at a wiki's address, and a page that is not there show the same
  "not found" as the rest of the app. An open wiki the reader has not joined
  shows its title with a way to join.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import WikiJoin from '$lib/features/wiki/WikiJoin.svelte';
  import WikiScreen from '$lib/features/wiki/WikiScreen.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { firstPage } from '$lib/logic/page-tree.ts';
  import { shownWiki, WikiStore, type Workspace } from '$lib/state/index.ts';

  import NotFound from './NotFound.svelte';
  import type { Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

  const wikiId = $derived(router.route.wikiId);
  const pageId = $derived(router.route.pageId);

  /** The wiki of the address, made anew for another one. */
  const wiki = $derived(wikiId ? new WikiStore(workspace, wikiId) : undefined);
  // Read while it is shown, and closed when it goes. The board shown before lets go.
  $effect(() => {
    const store = wiki;
    if (!store) return;
    const holder = shownWiki(workspace);
    untrack(() => {
      workspace.closeBoard();
      holder.current = store;
      void store.load();
    });
    return () => {
      store.close();
      if (holder.current === store) holder.current = undefined;
    };
  });

  // Without a page in the address: the first one, in place of the wiki's address.
  $effect(() => {
    if (wiki?.status !== 'ready' || pageId) return;
    const first = firstPage(wiki.pages);
    if (first) router.openPage(first.id, { replace: true });
  });

  /** The reader joined the wiki here: the wiki's screen takes the focus, where the button was. */
  let joined = $state(false);

  /** What the head of the page tells: a page shown that somebody else deleted. */
  let notice = $state<string | undefined>(undefined);

  // The page shown is gone: to the first page that is left (none: the empty wiki), told if not the reader's doing.
  $effect(() => {
    const removed = wiki?.openPage?.removed;
    if (!wiki || !removed) return;
    if (removed.by === 'others') {
      notice = m.wiki_page_gone({ title: removed.title || m.wiki_page_untitled() });
    }
    router.openPage(firstPage(wiki.pages)?.id, { replace: true });
  });

  // The page of the address is the one open; none without one.
  $effect(() => {
    const shown = wiki;
    const id = pageId;
    if (shown?.status === 'ready') untrack(() => shown.showPage(id));
  });
</script>

{#if !wiki || wiki.status === 'idle' || wiki.status === 'loading'}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if wiki.joinable && wiki.wiki}
  {@const store = wiki}
  <WikiJoin
    wiki={wiki.wiki}
    joining={wiki.joining}
    onJoin={() => {
      void store.join().then((done) => {
        joined = done;
      });
    }}
  />
{:else if wiki.missing}
  <NotFound {router} />
{:else if wiki.status === 'failed'}
  <div class="grid h-full place-items-center p-6 text-center">
    <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
  </div>
{:else}
  <WikiScreen
    {wiki}
    page={wiki.openPage}
    {notice}
    takeFocus={joined}
    onDismissNotice={() => {
      notice = undefined;
    }}
    onOpenPage={(id: string) => {
      router.openPage(id);
    }}
  >
    {#snippet missingPage()}
      <NotFound {router} />
    {/snippet}
  </WikiScreen>
{/if}
