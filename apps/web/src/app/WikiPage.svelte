<!--
  One wiki: its pages as a tree and the page the address names. Which wiki
  and page comes from the address; this keeps the stores and the address in
  step.

  A wiki's address without a page shows its first page, the address put in
  place (no entry of the history). A wiki not there for the reader, a board's
  identifier at a wiki's address, and a page that is not there show the same
  "not found" as the rest of the app. An open wiki the reader has not joined
  shows its title with a way to join.

  The page's body is the card's kind of body (`CardBody`), edited together in
  the page's room; while the wiki is shown the reader is on it, and on the
  page shown (the wiki's presence), as on a board and its card.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import CardBody from '$lib/features/card/CardBody.svelte';
  import WikiSettingsDialog from '$lib/features/wiki/WikiSettingsDialog.svelte';
  import WikiJoin from '$lib/features/wiki/WikiJoin.svelte';
  import WikiScreen from '$lib/features/wiki/WikiScreen.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { firstPage, withDescendants } from '$lib/logic/page-tree.ts';
  import {
    shownWiki,
    whileHere,
    WikiStore,
    type PageStore,
    type WikiLooks,
    type WikiPage,
    type Workspace,
  } from '$lib/state/index.ts';

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

  // Who is on the wiki, while it is shown and the reader is in it; and on which page.
  $effect(() => {
    const shown = wiki;
    if (shown?.status === 'ready' && !shown.missing && !shown.joinable) {
      return untrack(() => shown.presence.start());
    }
    return undefined;
  });
  $effect(() => {
    wiki?.presence.setCard(pageId);
  });

  /** The page the reader asked to delete, while it is asked whether to. */
  let deleting = $state.raw<WikiPage | undefined>(undefined);
  let deleteOpen = $state(false);
  /** How many pages are below the page to delete: they go with it. */
  const below = $derived(deleting && wiki ? withDescendants(wiki.pages, deleting.id).size - 1 : 0);
  let settingsOpen = $state(false);
  let deleteWikiOpen = $state(false);

  /** Deletes the wiki; the page goes on to the first board or wiki of the sidebar. */
  async function deleteWiki(store: WikiStore): Promise<void> {
    const done = await whileHere(workspace, async () => {
      await workspace.deleteBoard(store.wikiId);
      return true;
    });
    if (!done || workspace.boards.some((left) => left.id === store.wikiId)) return;
    const next = workspace.sidebar.flatMap((entry) => entry.boards)[0];
    if (next) router.openBoard(next);
  }

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
    pageHref={(id: string) => router.href({ ...router.route, pageId: id })}
    onDelete={(page: WikiPage) => {
      deleting = page;
      deleteOpen = true;
    }}
    onEditWiki={() => {
      settingsOpen = true;
    }}
    onDeleteWiki={() => {
      deleteWikiOpen = true;
    }}
    onOpenPage={(id: string) => {
      router.openPage(id);
    }}
  >
    {#snippet missingPage()}
      <NotFound {router} />
    {/snippet}
    {#snippet body(page: PageStore)}
      <!-- The page's body is edited together as a card's is, in the page's own room. -->
      <CardBody board={wiki} card={page} of="page" testId="page-body" />
    {/snippet}
  </WikiScreen>
  {@const store = wiki}
  {@const title = deleting?.title || m.wiki_page_untitled()}
  <ConfirmDialog
    bind:open={deleteOpen}
    title={m.wiki_delete_page_title()}
    description={below > 0
      ? m.wiki_delete_page_below({ title, count: below })
      : m.wiki_delete_page_alone({ title })}
    confirmLabel={m.shared_delete()}
    destructive
    onConfirm={() => {
      if (deleting) void store.deletePage(deleting.id);
    }}
  />
  {#if store.wiki}
    <WikiSettingsDialog
      bind:open={settingsOpen}
      wiki={store.wiki}
      onSave={(looks: WikiLooks) => void store.changeWiki(looks)}
    />
  {/if}
  <ConfirmDialog
    bind:open={deleteWikiOpen}
    title={m.shell_delete_wiki_title()}
    description={m.shell_delete_wiki_info({ title: store.wiki?.title || m.wiki_untitled() })}
    confirmLabel={m['DeleteBoardDialog.confirm-delete']()}
    destructive
    onConfirm={() => void deleteWiki(store)}
  />
{/if}
