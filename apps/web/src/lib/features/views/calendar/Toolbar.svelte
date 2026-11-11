<!--
  The row above the grid: back a month, on a month, today, and the month in
  view. The arrows and "today" come first and the name of the month after
  them: in a wide window the middle of the screen then falls on the name or
  on empty room, not on a button (docs/calendar-proposal.md, "한가운데 세로선").
  At the far end, the cards without a date and how many (question C16,
  answer A): the word where the pane has room, the icon and the count always.
-->
<script lang="ts">
  import CalendarOffIcon from '@lucide/svelte/icons/calendar-off';
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    /** The name of the month in view. */
    title: string;
    onstep: (by: number) => void;
    ontoday: () => void;
    /** How many cards have no date, whether their list is open, and what opens it. */
    undated: number;
    undatedOpen: boolean;
    onundated: () => void;
  }

  let { title, onstep, ontoday, undated, undatedOpen, onundated }: Props = $props();
</script>

<div
  data-testid="calendar-toolbar"
  class="flex h-12 shrink-0 items-center gap-0.5 border-b px-1.5 @columns/pane:px-3"
>
  <IconButton
    label={m.calendar_previous_month()}
    data-testid="calendar-previous"
    onclick={() => {
      onstep(-1);
    }}
  >
    <ChevronLeftIcon />
  </IconButton>
  <IconButton
    label={m.calendar_next_month()}
    data-testid="calendar-next"
    onclick={() => {
      onstep(1);
    }}
  >
    <ChevronRightIcon />
  </IconButton>
  <Button
    variant="outline"
    data-testid="calendar-today"
    class="roomy:h-touch roomy:px-3"
    onclick={ontoday}
  >
    {m.calendar_today()}
  </Button>
  <h2
    data-testid="calendar-period"
    aria-live="polite"
    class="ml-2 min-w-0 truncate font-semibold @columns/pane:text-lg"
  >
    {title}
  </h2>
  <div class="min-w-1 flex-1"></div>
  <Button
    variant="ghost"
    aria-label={m.calendar_undated_count({ count: undated })}
    aria-expanded={undatedOpen}
    data-testid="calendar-undated"
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
