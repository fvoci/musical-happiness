<!--
  One column: a heading and a list of cards that scrolls on its own.

  While a card is on its way the list shows a gap where it would land. The
  card itself stays in the list, hidden: a finger that drags it keeps sending
  its touch events to that element, so it must not leave the document.
-->
<script lang="ts">
  import CardFace from './CardFace.svelte';
  import type { Card, Column } from './data.ts';

  interface Props {
    column: Column;
    cards: readonly Card[];
    /** The card on its way, wherever it comes from. */
    draggedId: string | undefined;
    /** Where the gap is, counted among the cards without the dragged one; -1 for no gap. */
    gapAt: number;
    gapHeight: number;
    /** Drawn in the gap instead of an outline: the card the keyboard carries. */
    carried: Card | undefined;
    describedBy: string;
    onopen: (card: Card) => void;
  }

  let { column, cards, draggedId, gapAt, gapHeight, carried, describedBy, onopen }: Props =
    $props();

  type Entry = { kind: 'card'; card: Card } | { kind: 'gap' };
  const GAP_KEY = ' gap';

  const entries = $derived.by(() => {
    const list: Entry[] = [];
    let others = 0;
    for (const card of cards) {
      if (card.id !== draggedId) {
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
  data-column={column.id}
  aria-labelledby="column-{column.id}"
  class="flex max-h-full w-72 shrink-0 flex-col rounded-lg bg-muted"
>
  <h2 id="column-{column.id}" class="flex items-baseline justify-between px-3 pt-3 pb-1">
    <span class="font-semibold">{column.title}</span>
    <span class="text-xs text-muted-foreground">{cards.length}</span>
  </h2>
  <div
    data-drag-list={column.id}
    data-drag-scroll="y"
    class="flex min-h-16 flex-col gap-2 overflow-y-auto overscroll-y-contain p-2"
  >
    {#each entries as entry (entry.kind === 'gap' ? GAP_KEY : entry.card.id)}
      {#if entry.kind === 'card'}
        <div
          data-drag-item={entry.card.id}
          data-card={entry.card.id}
          role="button"
          tabindex="0"
          aria-describedby={describedBy}
          class={[
            'shrink-0 cursor-grab rounded-md select-none',
            entry.card.id === draggedId && 'hidden',
          ]}
          onclick={() => {
            onopen(entry.card);
          }}
          onkeydown={(event) => {
            if (event.key === 'Enter') onopen(entry.card);
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
          <CardFace card={carried} lifted />
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
