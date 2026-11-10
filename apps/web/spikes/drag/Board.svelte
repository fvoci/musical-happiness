<!--
  The spike's page: a board of made-up cards that can be dragged between and
  within its columns. The order of the cards is the only state; a drop
  replaces it, and nothing else ever changes it.
-->
<script lang="ts">
  import { Sortable } from '$lib/drag/sortable.svelte.ts';
  import { moveItem, type ListOrder } from '$lib/logic/drop-target.ts';

  import CardFace from './CardFace.svelte';
  import Column from './Column.svelte';
  import { fakeBoard, sizesFrom, type Card } from './data.ts';
  import { INSTRUCTIONS, MESSAGES } from './messages.ts';

  const board = fakeBoard(sizesFrom(location.search));

  let order = $state.raw<readonly ListOrder[]>(board.order);
  // Shown at the foot of the page, for a person trying it and for the browser tests.
  let moves = $state(0);
  let lastMove = $state('');
  let opened = $state('');

  const sortable = new Sortable({
    order: () => order,
    onDrop: ({ itemId, from, to }) => {
      order = moveItem(order, itemId, to);
      moves += 1;
      lastMove = `${itemId}: ${from.listId} ${from.index} to ${to.listId} ${to.index}`;
    },
    messages: MESSAGES,
    itemLabel: (id) => board.cards[id]?.title ?? id,
    listLabel: (id) => board.columns.find((column) => column.id === id)?.title ?? id,
  });

  const active = $derived(sortable.active);
  const dragged = $derived(active ? board.cards[active.itemId] : undefined);
  const overlay = $derived(sortable.overlay);

  function cardsOf(list: ListOrder | undefined): Card[] {
    return (list?.items ?? []).flatMap((id) => board.cards[id] ?? []);
  }
</script>

<div class="flex h-dvh flex-col">
  <header class="border-b px-4 py-3">
    <h1 class="text-lg font-semibold">Drag spike</h1>
    <p id="drag-help" class="text-xs text-muted-foreground">{INSTRUCTIONS}</p>
  </header>

  <div
    data-testid="board"
    data-drag-scroll="x"
    data-dragging={active?.mode}
    data-drop-list={active?.to.listId}
    data-drop-index={active?.to.index}
    class={[
      'flex min-h-0 flex-1 items-start gap-3 overflow-x-auto overflow-y-hidden p-3',
      active?.mode === 'pointer' && 'cursor-grabbing',
    ]}
    {@attach sortable.attach}
  >
    {#each board.columns as column, at (column.id)}
      <Column
        {column}
        cards={cardsOf(order[at])}
        draggedId={active?.itemId}
        gapAt={active?.to.listId === column.id ? active.to.index : -1}
        gapHeight={active?.height ?? 0}
        carried={active?.mode === 'keyboard' ? dragged : undefined}
        describedBy="drag-help"
        onopen={(card: Card) => {
          opened = card.id;
        }}
      />
    {/each}
  </div>

  <footer class="flex flex-wrap gap-x-4 border-t px-4 py-2 text-xs text-muted-foreground">
    <span>Moves: <span data-testid="moves">{moves}</span></span>
    <span>Last move: <span data-testid="last-move">{lastMove || 'none'}</span></span>
    <span>Opened: <span data-testid="opened">{opened || 'none'}</span></span>
  </footer>

  <div data-testid="announcer" class="sr-only" aria-live="assertive" aria-atomic="true">
    {sortable.announcement}
  </div>
</div>

{#if active?.mode === 'pointer' && overlay && dragged}
  <!-- Under the pointer, above everything, and never in the way of what is beneath. -->
  <div
    data-drag-overlay={dragged.id}
    aria-hidden="true"
    class="pointer-events-none fixed top-0 left-0 z-50 rotate-1 will-change-transform"
    style:width="{active.width}px"
    style:translate="{overlay.x}px {overlay.y}px"
  >
    <CardFace card={dragged} lifted />
  </div>
{/if}
