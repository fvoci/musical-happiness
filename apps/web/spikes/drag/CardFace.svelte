<!--
  What a card looks like. Drawn three times over: in its column, under the
  pointer while it is dragged, and at its would-be place while the keyboard
  carries it. Whatever makes it draggable is on the element around it.
-->
<script lang="ts">
  import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';

  import type { Card } from './data.ts';

  interface Props {
    card: Card;
    /** Off the board: under the pointer, or carried by the keyboard. */
    lifted?: boolean;
    /** How many cards travel with this one, itself included. Shown when more than one. */
    count?: number;
  }

  let { card, lifted = false, count = 1 }: Props = $props();
</script>

<div
  class={[
    'relative flex items-start gap-2 rounded-md border bg-card p-3 text-card-foreground',
    lifted ? 'shadow-lg ring-2 ring-ring' : 'shadow-xs',
  ]}
>
  <GripVerticalIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
  <div class="min-w-0">
    <p class="font-medium">{card.title}</p>
    {#if card.tag}
      <p class="mt-1 text-xs text-muted-foreground">{card.tag}</p>
    {/if}
  </div>
  {#if count > 1}
    <span
      data-carry-count
      class="absolute -end-2 -top-2 flex h-6 min-w-6 items-center justify-center rounded-full bg-primary px-1.5 text-xs font-semibold text-primary-foreground"
    >
      {count}
    </span>
  {/if}
</div>
