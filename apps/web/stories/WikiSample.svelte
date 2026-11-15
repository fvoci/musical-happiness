<!--
  A wiki of the example workspace: pages three levels deep with a long title,
  the deepest page shown, the pages above it open; or a wiki without pages; or one the reader may only read.
-->
<script lang="ts">
  import { onMount, untrack } from 'svelte';

  import BodyView from '$lib/editor/BodyView.svelte';
  import WikiScreen from '$lib/features/wiki/WikiScreen.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { firstPage } from '$lib/logic/page-tree.ts';
  import { WikiStore, type PageStore } from '$lib/state/index.ts';
  import { MemoryBackend } from '$lib/state/memory/backend.ts';

  import { exampleWorkspace } from './workspace.ts';

  let {
    empty = false,
    readOnly = false,
    manager = false,
  }: { empty?: boolean; readOnly?: boolean; manager?: boolean } = $props();

  const example = exampleWorkspace();
  let wiki = $state.raw<WikiStore | undefined>(undefined);
  let shown = $state<string | undefined>(undefined);

  async function seed(): Promise<{ store: WikiStore; deepest?: string }> {
    // Made by another member, so that the reader's role is the story's.
    const author = new MemoryBackend(example.world, 'example-member');
    const wikis = author.wiki;
    const made = await wikis.createWiki(example.world.workspace.id, {
      title: 'Handbook · 안내서',
      type: 'P',
    });
    await author.addMember(
      made.id,
      'example-reader',
      readOnly ? 'viewer' : manager ? 'admin' : 'editor',
    );
    let deepest: string | undefined;
    if (!empty) {
      const welcome = await wikis.createPage(made.id, { title: 'Welcome · 환영', icon: '👋' });
      const start = await wikis.createPage(made.id, {
        title: 'Getting started · 시작하기',
        parentId: welcome.id,
      });
      deepest = (await wikis.createPage(made.id, { title: 'Accounts · 계정', parentId: start.id }))
        .id;
      await wikis.createPage(made.id, {
        title: 'A page with a long title that does not fit · 한 줄에 다 들어가지 않는 긴 제목',
      });
      await wikis.createPage(made.id, { title: '' });
    }
    await example.workspace.load();
    const store = new WikiStore(example.workspace, made.id);
    await store.load();
    return { store, deepest };
  }

  onMount(() => {
    let store: WikiStore | undefined;
    void seed().then((ready) => {
      store = ready.store;
      wiki = ready.store;
      shown = ready.deepest ?? firstPage(ready.store.pages)?.id;
    });
    return () => {
      store?.close();
      example.workspace.close();
    };
  });

  // The page chosen is the one open, as the app's address makes it.
  $effect(() => {
    const store = wiki;
    const id = shown;
    if (store) untrack(() => store.showPage(id));
  });
</script>

<div class="@container/pane h-[36rem]" data-testid="wiki-example">
  {#if wiki?.status === 'ready'}
    <WikiScreen
      {wiki}
      page={wiki.openPage}
      pageHref={(id: string) => `#${id}`}
      onOpenPage={(id: string) => {
        shown = id;
      }}
      onEditWiki={() => undefined}
      onDeleteWiki={() => undefined}
      onOpenTrash={() => undefined}
    >
      {#snippet missingPage()}
        <p>{m.app_problem_load()}</p>
      {/snippet}
      {#snippet body(store: PageStore)}
        <BodyView doc={store.body} resolveUser={() => ''}>
          {#snippet empty()}
            <p class="text-muted-foreground">{m.wiki_page_body_empty()}</p>
          {/snippet}
        </BodyView>
      {/snippet}
    </WikiScreen>
  {:else}
    <p role="status">{m.app_loading()}</p>
  {/if}
</div>
