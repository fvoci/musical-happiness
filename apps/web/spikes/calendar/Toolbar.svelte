<!--
  The row above the grid: back, forward, "today", and what is in view (a
  month, or the days of a week). At the far end, the cards without a date
  (in the proposal that keeps them in a drawer) and the switch between month
  and week, where the pane has room for it; a narrow pane has that switch in
  the "…" menu of the view.

  The arrows and "today" come first and the title after them: in a wide
  window the middle of the screen then falls on the title or on empty room,
  not on a button.
-->
<script lang="ts">
  import CalendarOffIcon from '@lucide/svelte/icons/calendar-off';
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';

  import { Button } from '$lib/ui/button/index.js';

  import IconButton from '../mobile/IconButton.svelte';

  import { UNDATED } from './data.ts';
  import { monthTitle, rangeLabel } from './names.ts';
  import { SHOW } from './show.ts';
  import { T } from './text.ts';
  import { view, type Mode } from './view.svelte.ts';

  interface Props {
    /** The drawer of cards without a date is open. */
    undatedOpen: boolean;
    onundated: () => void;
  }

  let { undatedOpen, onundated }: Props = $props();

  const MODES: { mode: Mode; label: string; name: string }[] = [
    { mode: 'month', label: T.month, name: T.monthView },
    { mode: 'week', label: T.week, name: T.weekView },
  ];

  const first = $derived(view.week[0]);
  const last = $derived(view.week[6]);
  const title = $derived(
    view.mode === 'week' && first && last ? rangeLabel(first, last) : monthTitle(view.month),
  );
</script>

<div
  data-testid="calendar-toolbar"
  class="flex h-12 shrink-0 items-center gap-0.5 border-b px-1.5 @columns/pane:px-3"
>
  <IconButton
    label={view.byWeek ? T.previousWeek : T.previousMonth}
    data-testid="previous"
    onclick={() => {
      view.step(-1);
    }}
  >
    <ChevronLeftIcon />
  </IconButton>
  <IconButton
    label={view.byWeek ? T.nextWeek : T.nextMonth}
    data-testid="next"
    onclick={() => {
      view.step(1);
    }}
  >
    <ChevronRightIcon />
  </IconButton>
  <Button
    variant="outline"
    data-testid="go-today"
    class="pointer-coarse:h-touch pointer-coarse:px-3"
    onclick={() => {
      view.goToday();
    }}
  >
    {T.today}
  </Button>
  <h2
    data-testid="period"
    aria-live="polite"
    class="ml-2 min-w-0 truncate font-semibold @columns/pane:text-lg"
  >
    {title}
  </h2>
  <div class="flex-1"></div>

  {#if SHOW.undated === 'drawer'}
    <Button
      variant="ghost"
      aria-label={T.undatedCount(UNDATED.length)}
      aria-expanded={undatedOpen}
      data-testid="open-undated"
      class="shrink-0 gap-1.5 px-2 pointer-coarse:h-touch pointer-coarse:min-w-touch"
      onclick={onundated}
    >
      <CalendarOffIcon />
      <span class="hidden @columns/pane:inline">{T.undated}</span>
      <span
        class="inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-full bg-muted px-1 text-[0.6875rem] leading-none font-semibold ring-1 ring-input-border"
      >
        {UNDATED.length}
      </span>
    </Button>
  {/if}

  <div
    role="group"
    aria-label={T.span}
    data-testid="mode"
    class="ml-1 hidden shrink-0 overflow-hidden rounded-lg border @columns/pane:flex"
  >
    {#each MODES as { mode, label, name } (mode)}
      <button
        type="button"
        aria-label={name}
        aria-pressed={view.mode === mode}
        class="h-8 min-w-10 px-2.5 text-sm text-muted-foreground hover:bg-muted aria-pressed:bg-muted aria-pressed:font-medium aria-pressed:text-foreground pointer-coarse:h-touch pointer-coarse:min-w-touch"
        onclick={() => {
          view.mode = mode;
        }}
      >
        {label}
      </button>
    {/each}
  </div>
</div>
