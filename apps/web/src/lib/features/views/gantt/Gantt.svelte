<!--
  The Gantt view, to read: the cards of the view as bars on the days of one
  date property (`displayProperty`, as a calendar: the one the view names,
  or else the first kind of date of the board, or else when the card was
  made). A card of one day is a bar of one day. Where the view names a card
  relation as its dependencies, an arrow goes from each card to the cards
  that come after it.

  One box scrolls both ways. The titles of the cards are a column at its
  left that stays while the days scroll; the marks of the days are two rows
  at its top that stay while the cards scroll. A narrow pane has the same
  chart with a narrower column of titles.

  Lengths across are in rem and a row is as high as the style sheet says
  (`--gantt-row`, higher for a finger), so the chart follows the reader's
  text size (lib/logic/gantt.ts).

  Where the chart is scrolled to is the state of the screen only: another
  view, or a new load, starts with today in sight.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Room } from '$lib/layout/room.svelte.ts';
  import { dateOfInstant, type CalendarDate } from '$lib/logic/dates.ts';
  import { DAY_WIDTH, xOfDay, zoomOf } from '$lib/logic/gantt.ts';

  import { MinuteClock } from '../calendar/clock.svelte.ts';
  import type { ViewProps } from '../props.ts';

  import Arrows from './Arrows.svelte';
  import AxisHeader from './AxisHeader.svelte';
  import BarRow from './BarRow.svelte';
  import { GanttChart } from './chart.svelte.ts';
  import Toolbar from './Toolbar.svelte';

  let { board, view, cards, cardHref, onOpenCard }: ViewProps = $props();

  const locale = $derived(board.workspace.locale);
  const clock = new MinuteClock();
  /** Today, on the reader's calendar. */
  const today = $derived(dateOfInstant(clock.now, board.workspace.regional.timeZone));
  const zoom = $derived(zoomOf(view.fields.ganttZoom));

  /** The box the chart scrolls in, measured: the axis is at least as long as it is wide. */
  const box = new Room();

  /** The size of 1rem in pixels, at the reader's text size. */
  function rem(): number {
    return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  }

  /**
   * The days the box has room for, in steps of four weeks: a window dragged
   * wider does not work the chart out again at every pixel.
   */
  const minDays = $derived(Math.ceil(box.width / (DAY_WIDTH[zoom] * rem()) / 28) * 28);

  const chart = new GanttChart({
    board: () => board,
    view: () => view,
    cards: () => cards,
    today: () => today,
    zoom: () => zoom,
    minDays: () => minDays,
  });

  const axis = $derived(chart.axis);
  /** The middle of today, in rem from the start of the axis. */
  const todayX = $derived(xOfDay(axis, today) + axis.dayWidth / 2);

  /** The box that scrolls. Not reactive: read when something is brought into view. */
  let scroller: HTMLElement | undefined;

  /** Scrolls the days so that `date` is a third of the way into what is in sight. */
  function showDay(date: CalendarDate): void {
    if (!scroller) return;
    const titles = scroller.querySelector<HTMLElement>('[data-gantt-corner]')?.offsetWidth ?? 0;
    const x = (xOfDay(axis, date) + axis.dayWidth / 2) * rem();
    scroller.scrollLeft = Math.max(0, x - (scroller.clientWidth - titles) / 3);
  }

  /** The box, once it is drawn: it starts with today in sight. For `{@attach}`. */
  function attachScroller(element: HTMLElement): () => void {
    scroller = element;
    untrack(() => {
      showDay(today);
    });
    return () => {
      scroller = undefined;
    };
  }

  // ------------------------------------------------------------- focus

  /** The bar the Tab key stops on: the one last focused, the first until then. */
  let focused = $state<string | undefined>(undefined);
  const stop = $derived(
    focused !== undefined && chart.bars.has(focused) ? focused : chart.lines[0]?.id,
  );

  function focusBar(cardId: string | undefined): void {
    if (cardId === undefined) return;
    focused = cardId;
    scroller?.querySelector<HTMLElement>(`[data-bar="${CSS.escape(cardId)}"]`)?.focus();
  }

  /** The arrow keys walk the bars: up and down a row, Home and End to the first and last. */
  function barKeys(event: KeyboardEvent, cardId: string): void {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const lines = chart.lines;
    const at = lines.findIndex((line) => line.id === cardId);
    const to = { ArrowUp: at - 1, ArrowDown: at + 1, Home: 0, End: lines.length - 1 }[event.key];
    if (to === undefined) return;
    event.preventDefault();
    focusBar(lines[Math.min(lines.length - 1, Math.max(0, to))]?.id);
  }
</script>

<div
  data-testid="gantt"
  data-zoom={zoom}
  data-first={axis.first}
  data-last={axis.last}
  data-edges={chart.relations.edges.length}
  class="flex h-full min-h-0 flex-col [--gantt-row:2.25rem] [--gantt-title:7rem] @columns/pane:[--gantt-title:14rem] roomy:[--gantt-row:2.75rem]"
  style:--gantt-width="{axis.width}rem"
>
  <Toolbar
    ontoday={() => {
      showDay(today);
    }}
  />

  <!-- Measured here, outside the scrollbars of the box inside: the width does not depend on them. -->
  <div class="relative flex min-h-0 flex-1 flex-col" {@attach box.attach}>
    {#if box.measured}
      <div
        data-testid="gantt-scroller"
        class="min-h-0 flex-1 scroll-pt-12 scroll-pl-(--gantt-title) overflow-auto overscroll-contain"
        {@attach attachScroller}
      >
        <div
          class="relative flex min-h-full w-[calc(var(--gantt-title)+var(--gantt-width))] min-w-full flex-col"
        >
          <AxisHeader {axis} scale={chart.scale} {todayX} {today} {locale} />
          <div class="relative flex-1 pb-(--safe-bottom)">
            <!-- Under the rows: a line at each week or month, and the line of today. -->
            <div
              aria-hidden="true"
              class="pointer-events-none absolute inset-y-0 left-(--gantt-title) w-(--gantt-width)"
            >
              {#each chart.scale.lines as x (x)}
                <div class="absolute inset-y-0 w-px bg-border/60" style:left="{x}rem"></div>
              {/each}
              <div
                data-testid="gantt-today-line"
                data-day={today}
                class="absolute inset-y-0 w-0.5 -translate-x-1/2 bg-primary"
                style:left="{todayX}rem"
              ></div>
            </div>
            <Arrows arrows={chart.arrows} width={axis.width} rows={chart.rows.length} />
            <!-- The column of titles goes on under the last row: the lines of the days do not show through it. -->
            <div aria-hidden="true" class="pointer-events-none absolute inset-0">
              <div class="sticky left-0 h-full w-(--gantt-title) border-r bg-background"></div>
            </div>
            <ul aria-label={m.gantt_rows()} class="relative">
              {#each chart.lines as line (line.id)}
                <BarRow
                  {line}
                  current={line.id === stop}
                  href={cardHref(line.id)}
                  onopen={onOpenCard}
                  onkeys={barKeys}
                  onfocused={(cardId: string) => {
                    focused = cardId;
                  }}
                />
              {:else}
                <li
                  data-testid="gantt-empty"
                  class="sticky left-0 px-3 py-6 text-center text-muted-foreground"
                  style:width="{box.width}px"
                >
                  {m.gantt_empty()}
                </li>
              {/each}
            </ul>
          </div>
        </div>
      </div>
    {/if}
  </div>
</div>
