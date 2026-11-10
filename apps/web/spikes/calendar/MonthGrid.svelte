<!--
  The weeks of a month as a grid, each card a bar across its days. Drawn where
  the box of the calendar has room for it (34rem and up); the view of one week
  is the same grid with a single row as high as the box (`tall`).

  A week is one CSS grid: seven columns, a row for the numbers of the days,
  one row a bar, and the rest. The cells lie under the bars and span every
  row. How many rows of bars a cell has room for is worked out from its
  height; when a day has more cards than that, its last row says "+N".

  What can be pressed depends on the pointer, not on the width:

  - With a mouse, the number of a day picks the day and opens its cards, a
    bar opens its card, "+N" opens the cards of the day, and a "+" in the cell
    makes a card there (in the proposal that has it; shown when the cell is
    pointed at or holds the focus).
  - With a finger, the whole cell is one button that opens the cards of the
    day. A bar is 22 pixels high, which no finger hits; it is a picture.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';

  import type { CalendarDate } from '$lib/logic/dates.ts';

  import { coarse, rem } from '../mobile/layout.svelte.ts';

  import Bar from './Bar.svelte';
  import { SHOWN, entryOf } from './data.ts';
  import { foldWeek, inMonth, layWeek } from './grid.ts';
  import { dayLabel, dayLongLabel, dayNumber, weekdayName } from './names.ts';
  import { SHOW } from './show.ts';
  import { T } from './text.ts';
  import { view } from './view.svelte.ts';

  interface Props {
    weeks: readonly CalendarDate[][];
    /** One week as high as the box, every card shown, bars of two lines. */
    tall?: boolean;
    /** Menus are bottom sheets. */
    sheet: boolean;
    /** A day was picked and its cards are wanted. */
    onpick: (date: CalendarDate) => void;
  }

  let { weeks, tall = false, sheet, onpick }: Props = $props();

  /** The height of the rows together, in pixels. */
  let height = $state(0);

  // The row of numbers is 1.75rem and a bar with its gap 1.5rem; a week is never lower than 5.5rem.
  const lanes = $derived.by(() => {
    if (tall) return Infinity;
    const row = Math.max(rem(5.5), height / Math.max(1, weeks.length));
    return Math.max(1, Math.floor((row - rem(1.75) - 2) / rem(1.5)));
  });

  const rows = $derived(
    weeks.map((week) => {
      const folded = foldWeek(layWeek(week, view.spans), lanes);
      const deepest = Math.max(0, ...folded.shown.map((segment) => segment.lane + 1));
      return { week, ...folded, deepest };
    }),
  );

  const finger = $derived(coarse.current);
  const moved = entryOf(SHOWN.moved);

  function pick(date: CalendarDate): void {
    view.select(date);
    onpick(date);
  }
</script>

<div data-testid="month-grid" class="flex min-h-0 flex-1 flex-col">
  <!-- The names of the days. Every cell says its own date in full, so these are for the eye. -->
  <div aria-hidden="true" class="grid shrink-0 grid-cols-7 border-b text-xs text-muted-foreground">
    {#each weeks[0] ?? [] as date (date)}
      <div data-weekday class="truncate px-2 py-1.5">{weekdayName(date)}</div>
    {/each}
  </div>

  <div
    bind:clientHeight={height}
    class="grid min-h-0 flex-1 overflow-y-auto"
    style:grid-template-rows="repeat({weeks.length}, minmax(5.5rem, 1fr))"
  >
    {#each rows as row (row.week[0])}
      <div
        data-week={row.week[0]}
        class="relative grid grid-cols-7 border-b last:border-b-0"
        style:grid-template-rows={tall
          ? `1.75rem repeat(${Math.max(1, row.deepest)}, auto) minmax(0, 1fr)`
          : `1.75rem repeat(${lanes}, 1.5rem) minmax(0, 1fr)`}
      >
        {#each row.week as date, column (date)}
          {@const count = view.on(date).length}
          {@const today = date === view.today}
          {@const outside = !tall && !inMonth(date, view.month)}
          {@const picked = date === view.selected}
          {@const target = SHOW.dragging && date === SHOWN.dropOn}
          {@const cell = [
            'relative row-span-full row-start-1 flex min-w-0 items-start border-r text-left last:border-r-0',
            outside && 'bg-muted/50 text-muted-foreground',
            target && 'bg-ring/10 ring-2 ring-ring ring-inset',
          ]}
          {#snippet number()}
            <span
              class={[
                'flex h-6 min-w-6 items-center justify-center rounded-full px-1 text-xs tabular-nums',
                today && 'bg-primary font-semibold text-primary-foreground',
                picked && !today && 'bg-muted font-semibold ring-1 ring-input-border',
              ]}
            >
              {dayNumber(date)}
            </span>
          {/snippet}

          {#if finger}
            <button
              type="button"
              data-day={date}
              aria-label={T.cardsOn(dayLongLabel(date), count)}
              aria-current={today ? 'date' : undefined}
              aria-pressed={picked}
              class={[cell, 'p-0.5 active:bg-muted']}
              style:grid-column={column + 1}
              onclick={() => {
                pick(date);
              }}
            >
              {@render number()}
            </button>
          {:else}
            <div
              data-day={date}
              class={[cell, 'group/cell justify-between p-0.5']}
              style:grid-column={column + 1}
            >
              <button
                type="button"
                aria-label={T.cardsOn(dayLongLabel(date), count)}
                aria-current={today ? 'date' : undefined}
                aria-pressed={picked}
                class="rounded-full hover:bg-muted"
                onclick={() => {
                  pick(date);
                }}
              >
                {@render number()}
              </button>
              {#if SHOW.create === 'plus'}
                <button
                  type="button"
                  aria-label={T.newCardOn(dayLabel(date))}
                  data-new-on={date}
                  class={[
                    'flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:opacity-100',
                    !(SHOW.creating && date === SHOWN.empty) &&
                      'can-hover:opacity-0 can-hover:group-hover/cell:opacity-100',
                    SHOW.creating && date === SHOWN.empty && 'bg-muted text-foreground',
                  ]}
                >
                  <PlusIcon class="size-4" />
                </button>
              {/if}
            </div>
          {/if}

          {#if row.more[column]}
            {@const hidden = row.more[column] ?? 0}
            <div
              class={['z-10 min-w-0 px-0.5', finger && 'pointer-events-none']}
              style:grid-column={column + 1}
              style:grid-row={lanes + 1}
            >
              {#if finger}
                <span class="flex h-5.5 items-center px-1.5 text-xs text-muted-foreground">
                  {T.moreOn(hidden)}
                </span>
              {:else}
                <button
                  type="button"
                  data-more={date}
                  aria-label={T.moreOnLabel(dayLabel(date), hidden)}
                  class="flex h-5.5 w-full items-center rounded-sm px-1.5 text-left text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
                  onclick={() => {
                    pick(date);
                  }}
                >
                  {T.moreOn(hidden)}
                </button>
              {/if}
            </div>
          {/if}

          {#if SHOW.creating && SHOW.create === 'cell' && !finger && date === SHOWN.empty}
            <!-- The cell was pressed: a card is there at once and waits for its title. -->
            <div class="z-10 min-w-0 px-0.5" style:grid-column={column + 1} style:grid-row="2">
              <input
                data-testid="new-card-title"
                aria-label={T.newCardOn(dayLabel(date))}
                placeholder={T.untitled}
                class="h-5.5 w-full rounded-sm bg-background px-1.5 text-xs ring-2 ring-ring outline-none placeholder:text-muted-foreground"
              />
            </div>
          {/if}

          {#if target && moved}
            <!-- The copy that follows the pointer, over the day it would land on. -->
            <div
              aria-hidden="true"
              data-testid="drag-copy"
              class="pointer-events-none z-30 w-[130%] translate-x-1/4 translate-y-3 rotate-2 self-center rounded-sm bg-background shadow-lg"
              style:grid-column={column + 1}
              style:grid-row="3"
            >
              <Bar entry={moved} pressable={false} {sheet} inert />
            </div>
          {/if}
        {/each}

        {#each row.shown as segment (segment.id)}
          {@const entry = entryOf(segment.id)}
          {#if entry}
            <div
              class={[
                'z-10 min-w-0 self-start',
                segment.fromBefore ? 'pl-0' : 'pl-0.5',
                segment.toAfter ? 'pr-0' : 'pr-0.5',
                tall && 'pb-1',
                finger && !tall && 'pointer-events-none',
              ]}
              style:grid-column="{segment.column + 1} / span {segment.length}"
              style:grid-row={segment.lane + 2}
            >
              <Bar
                {entry}
                fromBefore={segment.fromBefore}
                toAfter={segment.toAfter}
                {tall}
                pressable={!finger || tall}
                {sheet}
                opened={entry.id === SHOWN.moved && !segment.toAfter ? SHOW.cardMenu : 'none'}
                lifted={SHOW.dragging && entry.id === SHOWN.moved}
                handles={SHOW.handles && entry.id === SHOWN.long}
              />
            </div>
          {/if}
        {/each}
      </div>
    {/each}
  </div>
</div>
