<!--
  The month in a box too narrow for bars: a day is its number, and under it
  only how much is on it. A card of one day is a dot (three at most, then
  "+N"); a card of several days is a thin line across them (two rows at most;
  a third counts as a dot). Titles are in the list of the picked day, which
  the shell draws underneath.

  Every day is a button, whatever the pointer: seven of them across 360
  pixels are 51 wide, and 3.75rem high.
-->
<script lang="ts">
  import { SHOWN, entryOf } from './data.ts';
  import { inMonth, layWeek } from './grid.ts';
  import { dayLongLabel, dayNumber, weekdayName } from './names.ts';
  import { SHOW } from './show.ts';
  import { T } from './text.ts';
  import { view } from './view.svelte.ts';

  /** Rows of lines under the number. */
  const LINES = 2;
  const DOTS = 3;

  const rows = $derived(
    view.weeks.map((week) => {
      const ranges = view.spans.filter((span) => span.first !== span.last);
      // What does not fit in the rows of lines joins the dots.
      const lines = layWeek(week, ranges).filter((segment) => segment.lane < LINES);
      const days = week.map((date, column) => {
        const lined = lines.filter(
          (line) => column >= line.column && column < line.column + line.length,
        ).length;
        const count = view.on(date).length;
        return { date, count, loose: count - lined };
      });
      return { week, lines, days };
    }),
  );

  const moved = entryOf(SHOWN.moved);
</script>

<div data-testid="mini-month" class="shrink-0">
  <div aria-hidden="true" class="grid grid-cols-7 text-center text-xs text-muted-foreground">
    {#each view.weeks[0] ?? [] as date (date)}
      <div data-weekday class="py-1">{weekdayName(date)}</div>
    {/each}
  </div>

  {#each rows as row (row.week[0])}
    <div data-week={row.week[0]} class="relative grid grid-cols-7">
      {#each row.days as { date, count, loose } (date)}
        {@const today = date === view.today}
        {@const picked = date === view.selected}
        {@const target = SHOW.dragging && date === SHOWN.dropOn}
        <button
          type="button"
          data-day={date}
          aria-label={T.cardsOn(dayLongLabel(date), count)}
          aria-current={today ? 'date' : undefined}
          aria-pressed={picked}
          class={[
            'relative flex h-15 min-w-0 flex-col items-center rounded-lg pt-1',
            !inMonth(date, view.month) && 'text-muted-foreground',
            picked && 'bg-muted',
            target && 'bg-ring/10 ring-2 ring-ring ring-inset',
          ]}
          onclick={() => {
            view.select(date);
          }}
        >
          <span
            class={[
              'flex size-7 items-center justify-center rounded-full tabular-nums',
              today && 'bg-primary font-semibold text-primary-foreground',
              picked && !today && 'font-semibold',
            ]}
          >
            {dayNumber(date)}
          </span>
          <!-- Under the lines: what is on the day and has no line. -->
          <span class="mt-auto flex h-3 items-center gap-0.5 pb-1">
            <!-- eslint-disable-next-line @typescript-eslint/no-unused-vars -->
            {#each { length: Math.min(DOTS, loose) } as _, at (at)}
              <span class="size-1 rounded-full bg-muted-foreground"></span>
            {/each}
            {#if loose > DOTS}
              <span class="text-[0.6875rem] leading-none text-muted-foreground">
                +{loose - DOTS}
              </span>
            {/if}
          </span>

          {#if target && moved}
            <span
              aria-hidden="true"
              data-testid="drag-copy"
              class="pointer-events-none absolute top-7 left-1/2 z-30 flex h-7 w-36 -translate-x-1/2 rotate-2 items-center rounded-md border bg-background px-2 text-xs shadow-lg"
            >
              <span class="truncate">{moved.title}</span>
            </span>
          {/if}
        </button>
      {/each}

      <div
        aria-hidden="true"
        class="pointer-events-none absolute inset-x-0 top-8.5 grid grid-cols-7 gap-y-0.5"
      >
        {#each row.lines as line (line.id)}
          <span
            class={[
              'h-1 bg-foreground/40',
              line.fromBefore ? 'rounded-l-none' : 'ml-1.5 rounded-l-full',
              line.toAfter ? 'rounded-r-none' : 'mr-1.5 rounded-r-full',
            ]}
            style:grid-column="{line.column + 1} / span {line.length}"
            style:grid-row={line.lane + 1}
          ></span>
        {/each}
      </div>
    </div>
  {/each}
</div>
