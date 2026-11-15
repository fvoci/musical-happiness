<!--
  A wiki read by its public link (`/shared/<wiki>/<page>?r=<token>`, the page
  in the place a board's view has): its pages as a tree, and the page the
  address names (the first without one) as it reads. Nobody is named, there
  are no comments and no versions, and nothing changes. A reference to another
  page of the wiki opens it; the server sent nothing of anything else. In a
  wide pane the tree is a column beside the page; at a phone's width it is a
  list above it that opens and closes. The visit counts as an anonymous one
  on the wiki and its page; nothing of who else is there comes back.
-->
<script lang="ts">
  import ListTreeIcon from '@lucide/svelte/icons/list-tree';

  import BodyView from '$lib/editor/BodyView.svelte';
  import type { ReferenceDrawing } from '$lib/editor/reference-chip.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { pane } from '$lib/layout/room.svelte.ts';
  import { firstPage, pageRows, pageTree } from '$lib/logic/page-tree.ts';
  import {
    isBackendError,
    type Backend,
    type WikiPage,
    type WikiPageDetail,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import NotFound from './NotFound.svelte';
  import type { Router } from './router.svelte.ts';

  interface Props {
    backend: Backend;
    wikiId: string;
    router: Router;
  }

  let { backend, wikiId, router }: Props = $props();

  /** The tree: being read, read, or not read (to try again). */
  let status = $state<'loading' | 'ready' | 'failed'>('loading');
  let pages = $state.raw<WikiPage[]>([]);
  /** The page shown, read with its body; `null` for one that is not there. */
  let shown = $state.raw<WikiPageDetail | null | undefined>(undefined);
  /** The page shown could not be read (and was not found missing): to try again. */
  let pageFailed = $state(false);
  let treeOpen = $state(false);
  /** Readings begun of the tree and of a page: the latest of each is the one that counts. */
  let treeReadings = 0;
  let pageReadings = 0;
  const uid = $props.id();

  const pageId = $derived(router.route.viewId);
  const rows = $derived(pageRows(pageTree(pages), () => true));
  const narrow = $derived(pane.measured && !pane.columns);
  const hrefOf = (id: string): string =>
    router.href({ ...router.route, viewId: id, cardId: undefined });

  const references: ReferenceDrawing = {
    shown: (id) => {
      const page = pages.find((one) => one.id === id);
      return page ? { title: page.title, icon: page.icon, kind: 'page', href: hrefOf(id) } : null;
    },
    open: (id) => {
      router.go({ ...router.route, viewId: id, cardId: undefined });
    },
  };

  async function readTree(): Promise<void> {
    const wikis = backend.wiki;
    if (!wikis) return;
    treeReadings += 1;
    const reading = treeReadings;
    status = 'loading';
    try {
      const read = await wikis.pages(wikiId);
      if (reading !== treeReadings) return;
      [pages, status] = [read, 'ready'];
    } catch {
      if (reading === treeReadings) status = 'failed';
    }
  }

  async function readPage(id: string): Promise<void> {
    const wikis = backend.wiki;
    if (!wikis) return;
    pageReadings += 1;
    const reading = pageReadings;
    [shown, pageFailed] = [undefined, false];
    try {
      const page = await wikis.page(wikiId, id);
      if (reading === pageReadings) shown = page;
    } catch (error) {
      if (reading !== pageReadings) return;
      if (isBackendError(error) && error.status === 404) shown = null;
      else pageFailed = true;
    }
  }

  // The tree, once.
  $effect(() => {
    void readTree();
  });

  // Without a page in the address: the first one, in place of the wiki's address.
  $effect(() => {
    if (status !== 'ready' || pageId) return;
    const first = firstPage(pages);
    if (first) router.go({ ...router.route, viewId: first.id }, { replace: true });
  });

  // The page of the address, read with its body.
  $effect(() => {
    const id = pageId;
    if (id) void readPage(id);
  });

  // The visit: counted on the wiki and its page, anonymously.
  $effect(() => {
    backend.presence.setPresence({ workspaceId: '', boardId: wikiId, cardId: pageId ?? '' });
  });
  $effect(() => () => {
    backend.presence.setPresence(null);
  });
</script>

{#snippet problem(retry: () => void)}
  <div class="grid h-full place-items-center p-6 text-center">
    <div>
      <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
      <Button class="mt-4 roomy:min-h-touch" onclick={retry}>{m.app_retry()}</Button>
    </div>
  </div>
{/snippet}

{#snippet tree()}
  <nav
    id="{uid}-tree"
    aria-label={m.wiki_pages()}
    class={[narrow && 'max-h-[50dvh] overflow-y-auto overscroll-contain']}
    data-testid="shared-page-tree"
  >
    <ul class="grid gap-0.5 p-2">
      {#each rows as row (row.page.id)}
        <li style:padding-left="{row.depth * 0.75}rem">
          <a
            href={hrefOf(row.page.id)}
            aria-current={row.page.id === pageId ? 'page' : undefined}
            class="flex min-h-8 items-center gap-1.5 truncate rounded-md px-2 hover:bg-muted aria-[current=page]:bg-muted aria-[current=page]:font-medium roomy:min-h-touch"
            onclick={(event) => {
              if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
              event.preventDefault();
              treeOpen = false;
              router.go({ ...router.route, viewId: row.page.id, cardId: undefined });
            }}
          >
            {#if row.page.icon}<span aria-hidden="true">{row.page.icon}</span>{/if}
            <span class="truncate">{row.page.title || m.wiki_page_untitled()}</span>
          </a>
        </li>
      {/each}
    </ul>
  </nav>
{/snippet}

{#if status === 'loading'}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if status === 'failed'}
  {@render problem(() => void readTree())}
{:else}
  <div class={['flex min-h-0 flex-1', narrow && 'flex-col']} data-testid="shared-wiki">
    {#if narrow}
      <div class="shrink-0 border-b">
        <button
          type="button"
          class="flex min-h-touch w-full items-center gap-2 px-4 text-sm text-muted-foreground"
          aria-expanded={treeOpen}
          aria-controls="{uid}-tree"
          onclick={() => {
            treeOpen = !treeOpen;
          }}
        >
          <ListTreeIcon class="size-4" aria-hidden="true" />
          {m.wiki_pages()}
        </button>
        {#if treeOpen}{@render tree()}{/if}
      </div>
    {:else}
      <div class="w-64 shrink-0 overflow-y-auto border-r">{@render tree()}</div>
    {/if}
    <article class="min-w-0 flex-1 overflow-y-auto px-4 py-6" data-testid="shared-page">
      {#if pages.length === 0}
        <p class="text-muted-foreground">{m.wiki_no_pages()}</p>
      {:else if shown === null}
        <NotFound {router} />
      {:else if pageFailed && pageId}
        {@const id = pageId}
        {@render problem(() => void readPage(id))}
      {:else if shown}
        <div class="mx-auto max-w-3xl">
          <h2 class="mb-4 text-2xl font-semibold break-words" data-testid="page-title">
            {#if shown.icon}<span aria-hidden="true">{shown.icon} </span>{/if}{shown.title ||
              m.wiki_page_untitled()}
          </h2>
          <div data-testid="page-body">
            <BodyView doc={shown.body} {references}>
              {#snippet empty()}
                <p class="text-muted-foreground">{m.wiki_page_body_empty()}</p>
              {/snippet}
            </BodyView>
          </div>
        </div>
      {:else}
        <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
      {/if}
    </article>
  </div>
{/if}
