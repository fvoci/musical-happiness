<!-- A stand-in until the header of a view is built: the views as links, a search field, "New". -->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardView } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';

  interface Props {
    board: BoardStore;
    view: BoardView;
    /** The search text of the view. Bindable. */
    search: string;
    viewHref: (viewId: string) => string;
    onOpenView: (viewId: string) => void;
    /** Makes a card the view will show, and opens it. */
    onAddCard: () => void;
  }

  let { board, view, search = $bindable(), viewHref, onOpenView, onAddCard }: Props = $props();
</script>

<div
  class="flex shrink-0 flex-wrap items-center gap-2 border-b px-4 py-2"
  data-testid="view-header"
>
  <nav class="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
    {#each board.orderedViews as each (each.id)}
      <a
        class="rounded-md px-2 py-1 whitespace-nowrap hover:bg-muted aria-[current=page]:bg-muted aria-[current=page]:font-medium"
        href={viewHref(each.id)}
        aria-current={each.id === view.id ? 'page' : undefined}
        onclick={(event) => {
          event.preventDefault();
          onOpenView(each.id);
        }}
      >
        {each.title || m['Sidebar.untitled-view']()}
      </a>
    {/each}
  </nav>
  <Input
    class="w-40"
    type="search"
    aria-label={m['ViewHeader.search-text']()}
    placeholder={m['ViewHeader.search-text']()}
    bind:value={search}
  />
  {#if board.canEdit}
    <Button onclick={onAddCard}>{m['ViewHeader.new']()}</Button>
  {/if}
</div>
