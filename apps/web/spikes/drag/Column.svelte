<!--
  One column: a heading and a list of cards that scrolls on its own.

  The column is an item of the board's row of columns, dragged by its heading
  (scope "column"); its cards are items of the list inside it (no scope).

  While cards are on their way the list shows a gap where they would land.
  The cards themselves stay in the list, hidden: a finger that drags one keeps
  sending its touch events to that element, so it must not leave the document.
  The same goes for the column while a pointer drags it.
-->
<script lang="ts">
  import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';

  import CardFace from './CardFace.svelte';
  import type { Card, Column } from './data.ts';

  interface Props {
    column: Column;
    cards: readonly Card[];
    /** The cards on their way, wherever they come from. */
    draggedIds: readonly string[];
    /** Where the gap is, counted among the cards without the dragged ones; -1 for no gap. */
    gapAt: number;
    gapHeight: number;
    /** Drawn in the gap instead of an outline: the card the keyboard carries. */
    carried: Card | undefined;
    /** How many cards are on their way with it. */
    carriedCount: number;
    selectedIds: readonly string[];
    /** A pointer drags this column: it stays in the document, out of sight. */
    hidden: boolean;
    /** The keyboard carries this column: its heading is the stand-in that holds the focus. */
    lifted: boolean;
    describedBy: string;
    headingDescribedBy: string;
    onactivate: (card: Card) => void;
  }

  let {
    column,
    cards,
    draggedIds,
    gapAt,
    gapHeight,
    carried,
    carriedCount,
    selectedIds,
    hidden,
    lifted,
    describedBy,
    headingDescribedBy,
    onactivate,
  }: Props = $props();

  type Entry = { kind: 'card'; card: Card } | { kind: 'gap' };
  const GAP_KEY = ' gap';

  const entries = $derived.by(() => {
    const list: Entry[] = [];
    let others = 0;
    for (const card of cards) {
      if (!draggedIds.includes(card.id)) {
        if (others === gapAt) list.push({ kind: 'gap' });
        others += 1;
      }
      list.push({ kind: 'card', card });
    }
    if (others === gapAt) list.push({ kind: 'gap' });
    return list;
  });
</script>

<section
  data-drag-item={column.id}
  data-drag-scope="column"
  data-column={column.id}
  aria-labelledby="column-{column.id}"
  class={[
    'flex max-h-full w-72 shrink-0 flex-col rounded-lg bg-muted',
    hidden && 'hidden',
    lifted && 'ring-2 ring-ring',
  ]}
>
  <div
    data-drag-handle
    data-drag-scope="column"
    data-drag-carried={lifted ? column.id : undefined}
    data-column-heading={column.id}
    role="button"
    tabindex="0"
    aria-describedby={headingDescribedBy}
    class="flex cursor-grab items-center gap-1 rounded-t-lg px-2 pt-3 pb-1 outline-none select-none focus-visible:ring-2 focus-visible:ring-ring"
  >
    <GripVerticalIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    <h2 id="column-{column.id}" class="min-w-0 flex-1 font-semibold">{column.title}</h2>
    <span class="pe-1 text-xs text-muted-foreground">{cards.length}</span>
  </div>
  <div
    data-drag-list={column.id}
    data-drag-scroll="y"
    class="flex min-h-16 flex-col gap-2 overflow-y-auto overscroll-y-contain p-2"
  >
    {#each entries as entry (entry.kind === 'gap' ? GAP_KEY : entry.card.id)}
      {#if entry.kind === 'card'}
        {@const selected = selectedIds.includes(entry.card.id)}
        <div
          data-drag-item={entry.card.id}
          data-card={entry.card.id}
          data-selected={selected ? '' : undefined}
          role="button"
          tabindex="0"
          aria-describedby={describedBy}
          class={[
            'shrink-0 cursor-grab rounded-md select-none',
            draggedIds.includes(entry.card.id) && 'hidden',
            selected && 'ring-2 ring-primary',
          ]}
          onclick={() => {
            onactivate(entry.card);
          }}
          onkeydown={(event) => {
            if (event.key === 'Enter') onactivate(entry.card);
          }}
        >
          <CardFace card={entry.card} />
        </div>
      {:else if carried}
        <div
          data-drag-carried={carried.id}
          role="button"
          tabindex="0"
          aria-describedby={describedBy}
          class="shrink-0 rounded-md select-none"
        >
          <CardFace card={carried} lifted count={carriedCount} />
        </div>
      {:else}
        <div
          data-drag-gap
          aria-hidden="true"
          class="shrink-0 rounded-md border-2 border-dashed border-input-border"
          style:height="{gapHeight}px"
        ></div>
      {/if}
    {/each}
  </div>
</section>
