<!--
  The row above the grid: back a month, on a month, today, and the month in
  view. The arrows and "today" come first and the name of the month after
  them: in a wide window the middle of the screen then falls on the name or
  on empty room, not on a button (docs/calendar-proposal.md, "한가운데 세로선").
-->
<script lang="ts">
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
  }

  let { title, onstep, ontoday }: Props = $props();
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
</div>
