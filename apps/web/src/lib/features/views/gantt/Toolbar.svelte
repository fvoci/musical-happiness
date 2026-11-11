<!--
  The row above the chart: "Today" brings today into view, and two buttons
  zoom out and in a step (days, weeks, months), the step said between them
  where the pane has room.

  At the far end, how many relations are out of order (a card starts before
  a card it comes after has ended) and how many cards are on a loop, each
  only where there are any, and the cards without a date, as the calendar
  has them: the word where the pane has room, the mark and the count always.
  The first two are buttons that go to the next such card.
-->
<script lang="ts">
  import CalendarOffIcon from '@lucide/svelte/icons/calendar-off';
  import RepeatIcon from '@lucide/svelte/icons/repeat';
  import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
  import ZoomInIcon from '@lucide/svelte/icons/zoom-in';
  import ZoomOutIcon from '@lucide/svelte/icons/zoom-out';
  import type { Component } from 'svelte';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { GANTT_ZOOMS } from '$lib/logic/gantt.ts';
  import type { GanttZoom } from '$lib/logic/wire.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    ontoday: () => void;
    /** How many relations are out of order, and what goes to the next card of one. */
    late: number;
    onlate: () => void;
    /** How many cards with a bar are on a loop, and what goes to the next. */
    loops: number;
    onloop: () => void;
    zoom: GanttZoom;
    onzoom: (zoom: GanttZoom) => void;
    /** How many cards have no date, whether their list is open, and what opens it. */
    undated: number;
    undatedOpen: boolean;
    onundated: () => void;
    /** "No date" itself, for the focus to come back to. Bindable. */
    undatedButton?: HTMLElement | null;
  }

  let {
    ontoday,
    late,
    onlate,
    loops,
    onloop,
    zoom,
    onzoom,
    undated,
    undatedOpen,
    onundated,
    undatedButton = $bindable(null),
  }: Props = $props();

  const ZOOM_NAMES: Record<GanttZoom, () => string> = {
    day: m.gantt_zoom_day,
    week: m.gantt_zoom_week,
    month: m.gantt_zoom_month,
  };
  /** The steps either side of the one in use: closer is earlier in the list. */
  const closer = $derived(GANTT_ZOOMS[GANTT_ZOOMS.indexOf(zoom) - 1]);
  const farther = $derived(GANTT_ZOOMS[GANTT_ZOOMS.indexOf(zoom) + 1]);
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
  <!-- At an end of the steps a button stays where it is and does nothing: the focus is not lost. -->
  <IconButton
    label={m.gantt_zoom_out()}
    data-testid="gantt-zoom-out"
    aria-disabled={farther === undefined}
    class="aria-disabled:opacity-40"
    onclick={() => {
      if (farther) onzoom(farther);
    }}
  >
    <ZoomOutIcon />
  </IconButton>
  <span
    data-testid="gantt-zoom"
    aria-live="polite"
    class="sr-only text-sm text-muted-foreground @columns/pane:not-sr-only"
  >
    <span class="sr-only">{m.gantt_zoom_step({ step: ZOOM_NAMES[zoom]() })}</span>
    <span aria-hidden="true">{ZOOM_NAMES[zoom]()}</span>
  </span>
  <IconButton
    label={m.gantt_zoom_in()}
    data-testid="gantt-zoom-in"
    aria-disabled={closer === undefined}
    class="aria-disabled:opacity-40"
    onclick={() => {
      if (closer) onzoom(closer);
    }}
  >
    <ZoomInIcon />
  </IconButton>
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
  <Button
    bind:ref={undatedButton}
    variant="ghost"
    aria-label={m.calendar_undated_count({ count: undated })}
    aria-expanded={undatedOpen}
    data-testid="gantt-undated"
    class="shrink-0 gap-1.5 px-2 roomy:h-touch roomy:min-w-touch"
    onclick={onundated}
  >
    <CalendarOffIcon />
    <span class="hidden @columns/pane:inline">{m.calendar_undated()}</span>
    <span
      class="inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-full bg-muted px-1 text-[0.6875rem] leading-none font-semibold ring-1 ring-input-border"
    >
      {undated}
    </span>
  </Button>
</div>
