<!--
  A wiki: its pages, and the page that is open. A wiki without pages says so,
  with a way to make the first one for whoever may write in it.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';

  import FilePlusIcon from '@lucide/svelte/icons/file-plus';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { whileHere, type PageStore, type WikiStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    wiki: WikiStore;
    page: PageStore | undefined;
    onOpenPage: (pageId: string) => void;
    /** What a page that is not there shows. */
    missingPage: Snippet;
  }

  let { wiki, page, onOpenPage, missingPage }: Props = $props();

  async function firstPage(): Promise<void> {
    const made = await whileHere(wiki.workspace, () => wiki.createPage({}));
    if (made) onOpenPage(made.id);
  }
</script>

<div class="flex h-full min-h-0 flex-col outline-none" tabindex="-1" data-testid="wiki-screen">
  {#if wiki.pages.length === 0}
    <div class="grid h-full place-items-center p-6 text-center" data-testid="wiki-empty">
      <div class="flex max-w-md flex-col items-center gap-3">
        <p class="text-muted-foreground">{m.wiki_no_pages()}</p>
        {#if wiki.canEdit}
          <Button
            class="roomy:min-h-touch"
            data-testid="wiki-first-page"
            onclick={() => void firstPage()}
          >
            <FilePlusIcon aria-hidden="true" />
            {m.wiki_first_page()}
          </Button>
        {/if}
      </div>
    </div>
  {:else if page?.missing}
    {@render missingPage()}
  {:else if page}
    <h2 class="p-6 text-2xl font-semibold" data-testid="page-title">
      {page.page?.title || m.wiki_page_untitled()}
    </h2>
  {/if}
</div>
