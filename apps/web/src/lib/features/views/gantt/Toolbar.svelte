<!--
  The row above the chart: "Today" brings today into view. At the far end,
  how many relations are out of order (a card starts before a card it comes
  after has ended) and how many cards are on a loop, each only where there
  are any: the word where the pane has room, the mark and the count always.
  Each is a button that goes to the next such card.
-->
<script lang="ts">
  import RepeatIcon from '@lucide/svelte/icons/repeat';
  import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
  import type { Component } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    ontoday: () => void;
    /** How many relations are out of order, and what goes to the next card of one. */
    late: number;
    onlate: () => void;
    /** How many cards with a bar are on a loop, and what goes to the next. */
    loops: number;
    onloop: () => void;
  }

  let { ontoday, late, onlate, loops, onloop }: Props = $props();
</script>

{#snippet problem(
  id: string,
  Mark: Component<{ class?: string }>,
  word: string,
  name: string,
  count: number,
  onclick: () => void,
)}
  <Button
    variant="ghost"
    aria-label={name}
    data-testid={id}
    class="shrink-0 gap-1.5 px-2 text-destructive roomy:h-touch roomy:min-w-touch"
    {onclick}
  >
    <Mark />
    <span class="hidden @columns/pane:inline">{word}</span>
    <span
      data-count
      class="inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-full px-1 text-[0.6875rem] leading-none font-semibold ring-1 ring-current"
    >
      {count}
    </span>
  </Button>
{/snippet}

<div
  data-testid="gantt-toolbar"
  class="flex h-12 shrink-0 items-center gap-1 border-b px-1.5 @columns/pane:px-3"
>
  <Button
    variant="outline"
    data-testid="gantt-today"
    class="roomy:h-touch roomy:px-3"
    onclick={ontoday}
  >
    {m.calendar_today()}
  </Button>
  <div class="min-w-1 flex-1"></div>
  {#if late > 0}
    {@render problem(
      'gantt-late',
      TriangleAlertIcon,
      m.gantt_late(),
      m.gantt_late_next({ count: late }),
      late,
      onlate,
    )}
  {/if}
  {#if loops > 0}
    {@render problem(
      'gantt-loops',
      RepeatIcon,
      m.gantt_loop(),
      m.gantt_loop_next({ count: loops }),
      loops,
      onloop,
    )}
  {/if}
</div>
