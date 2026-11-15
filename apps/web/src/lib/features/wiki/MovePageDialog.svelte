<!--
  "Move…": the page goes below the page chosen, last among its pages, or
  last at the top of the wiki, with every page below it. Every page of the
  wiki is listed, unfolded, as a group of radio buttons the arrows go
  through; the page itself and the pages below it cannot be chosen. The
  dialog opens on where the page is now.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { PagePlace } from '$lib/logic/page-moves.ts';
  import { pageRows, withDescendants } from '$lib/logic/page-tree.ts';
  import type { WikiPage, WikiStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';

  interface Props {
    open: boolean;
    wiki: WikiStore;
    /** The page to move, as it is now; the dialog shows nothing without one. */
    pageId: string | undefined;
    onMove: (pageId: string, to: PagePlace) => void;
  }

  let { open = $bindable(), wiki, pageId, onMove }: Props = $props();

  const uid = $props.id();
  /** The page chosen to go below; empty for the top of the wiki. */
  let parentId = $state('');

  const page = $derived(wiki.page(pageId));
  const rows = $derived(pageRows(wiki.tree, () => true));
  /** It and the pages below it: none of them is a place for it. */
  const moving = $derived(
    pageId === undefined ? new Set<string>() : withDescendants(wiki.pages, pageId),
  );
  const titleOf = (one: WikiPage): string => one.title || m.wiki_page_untitled();

  /** Each opening starts from where the page is. */
  function opened(): void {
    untrack(() => {
      parentId = page?.parentId ?? '';
    });
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    open = false;
    if (!page || page.parentId === parentId) return;
    onMove(page.id, { parentId });
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content data-testid="move-page" {@attach opened}>
    {#if page}
      <form class="grid min-h-0 gap-4" onsubmit={submit}>
        <Dialog.Header>
          <Dialog.Title>{m.wiki_move_title({ title: titleOf(page) })}</Dialog.Title>
          <Dialog.Description id="{uid}-about">{m.wiki_move_description()}</Dialog.Description>
        </Dialog.Header>
        <div
          role="radiogroup"
          aria-labelledby="{uid}-about"
          class="max-h-[50dvh] overflow-y-auto overscroll-contain rounded-md border p-1"
        >
          <label
            class="flex min-h-8 cursor-pointer items-center gap-2 rounded-sm px-2 hover:bg-muted roomy:min-h-touch"
          >
            <input type="radio" name="{uid}-parent" value="" bind:group={parentId} />
            <span class="font-medium">{m.wiki_move_top()}</span>
          </label>
          {#each rows as row (row.page.id)}
            {@const allowed = !moving.has(row.page.id)}
            <label
              class={[
                'flex min-h-8 items-center gap-2 rounded-sm px-2 roomy:min-h-touch',
                allowed ? 'cursor-pointer hover:bg-muted' : 'text-muted-foreground',
              ]}
              style:padding-left="{0.5 + (row.depth + 1) * 1}rem"
              data-target={row.page.id}
            >
              <input
                type="radio"
                name="{uid}-parent"
                value={row.page.id}
                disabled={!allowed}
                aria-label={m.wiki_move_target({ title: titleOf(row.page), level: row.depth + 1 })}
                bind:group={parentId}
              />
              {#if row.page.icon}<span aria-hidden="true">{row.page.icon}</span>{/if}
              <span class="min-w-0 truncate">{titleOf(row.page)}</span>
            </label>
          {/each}
        </div>
        <Dialog.Footer>
          <Button
            type="button"
            variant="outline"
            onclick={() => {
              open = false;
            }}
          >
            {m.shared_cancel()}
          </Button>
          <Button type="submit" disabled={page.parentId === parentId}>
            {m.wiki_move_confirm()}
          </Button>
        </Dialog.Footer>
      </form>
    {/if}
  </Dialog.Content>
</Dialog.Root>
