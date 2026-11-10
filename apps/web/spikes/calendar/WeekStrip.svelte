<!--
  One week as a row of seven days: the name of the day, its number, and a dot
  when something is on it. It stands above the list of days in a narrow box
  and says where in the month the list is; a day in it picks that day.
-->
<script lang="ts">
  import { dayLongLabel, dayNumber, weekdayName } from './names.ts';
  import { T } from './text.ts';
  import { view } from './view.svelte.ts';
</script>

<div
  data-testid="week-strip"
  role="group"
  aria-label={T.weekStrip}
  class="grid shrink-0 grid-cols-7 border-b pb-1"
>
  {#each view.week as date (date)}
    {@const today = date === view.today}
    {@const picked = date === view.selected}
    {@const count = view.on(date).length}
    <button
      type="button"
      data-day={date}
      aria-label={T.cardsOn(dayLongLabel(date), count)}
      aria-current={today ? 'date' : undefined}
      aria-pressed={picked}
      class={['flex h-15 min-w-0 flex-col items-center rounded-lg pt-1', picked && 'bg-muted']}
      onclick={() => {
        view.select(date);
      }}
    >
      <span data-weekday class="text-xs text-muted-foreground">{weekdayName(date)}</span>
      <span
        class={[
          'flex size-7 items-center justify-center rounded-full tabular-nums',
          today && 'bg-primary font-semibold text-primary-foreground',
          picked && !today && 'font-semibold',
        ]}
      >
        {dayNumber(date)}
      </span>
      <span class={['mt-0.5 size-1 rounded-full', count > 0 && 'bg-muted-foreground']}></span>
    </button>
  {/each}
</div>
