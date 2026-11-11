<!--
  The Gantt view, to read: the cards of the view as bars on the days of one
  date property (`displayProperty`, as a calendar: the one the view names,
  or else the first kind of date of the board, or else when the card was
  made). A card of one day is a bar of one day. Where the view names a card
  relation as its dependencies, an arrow goes from each card to the cards
  that come after it; dates are never moved. A card that starts before a
  card it comes after has ended is out of order (ending and starting on the
  same day is in order): its arrow is dashed, its title carries a mark, its
  bar says so in words, and the row above the chart counts them.
  Cards on a loop are marked and counted the same way.

  One box scrolls both ways. The titles of the cards are a column at its
  left that stays while the days scroll; the marks of the days are two rows
  at its top that stay while the cards scroll. A narrow pane has the same
  chart with a narrower column of titles.

  Lengths across are in rem and a row is as high as the style sheet says
  (`--gantt-row`, higher for a finger), so the chart follows the reader's
  text size (lib/logic/gantt.ts).

  The view is grouped as a table is (`groupById`): a heading before the cards
  of each group. A card without a value for the date has no row: the row
  above the chart counts them and lists them on asking, as the calendar does
  (a column at the right, or a sheet on a compact page).

  The zoom is the view's (`ganttZoom`), stored where the reader may change
  the view and kept for the visit where not. Where the chart is scrolled to
  is the state of the screen only: another view, or a new load, starts with
  today in sight.
-->
<script lang="ts">
  import { tick, untrack } from 'svelte';

  import BottomSheet from '$lib/components/sheet/BottomSheet.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { overlayPresentation } from '$lib/layout/presentation.ts';
  import { page, Room } from '$lib/layout/room.svelte.ts';
  import { calendarDate, dateOfInstant, daysBetween, type CalendarDate } from '$lib/logic/dates.ts';
  import { DAY_WIDTH, xOfDay, zoomOf, type Axis } from '$lib/logic/gantt.ts';
  import type { GanttZoom } from '$lib/logic/wire.ts';

  import { MinuteClock } from '../calendar/clock.svelte.ts';
  import Undated from '../calendar/Undated.svelte';
  import type { ViewProps } from '../props.ts';

  import Arrows from './Arrows.svelte';
  import AxisHeader from './AxisHeader.svelte';
  import BarRow from './BarRow.svelte';
  import { GanttChart } from './chart.svelte.ts';
  import GroupRow from './GroupRow.svelte';
  import Toolbar from './Toolbar.svelte';

  let { board, view, cards, cardHref, onOpenCard }: ViewProps = $props();

  const locale = $derived(board.workspace.locale);
  const clock = new MinuteClock();
  /** Today, on the reader's calendar. */
  const today = $derived(dateOfInstant(clock.now, board.workspace.regional.timeZone));
  /** The zoom a reader who may not change the view chose, for this visit only. */
  let ownZoom = $state<GanttZoom | undefined>(undefined);
  const zoom = $derived((board.canEdit ? undefined : ownZoom) ?? zoomOf(view.fields.ganttZoom));

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

  /** How wide the days in sight are, in pixels: the box less the column of titles. */
  function daysRoom(): number {
    if (!scroller) return 0;
    const titles = scroller.querySelector<HTMLElement>('[data-gantt-corner]')?.offsetWidth ?? 0;
    return scroller.clientWidth - titles;
  }

  /** Scrolls the days so that the middle of `date` is `part` of the way into what is in sight. */
  function showDay(date: CalendarDate, part = 1 / 3): void {
    if (!scroller) return;
    const x = (xOfDay(axis, date) + axis.dayWidth / 2) * rem();
    scroller.scrollLeft = Math.max(0, x - daysRoom() * part);
  }

  /** Another zoom: the view's where the reader may change the view, theirs for the visit where not. */
  function setZoom(next: GanttZoom): void {
    if (next === zoom) return;
    if (board.canEdit) void board.setViewFields(view.id, { ganttZoom: next }, 'ganttZoom');
    else ownZoom = next;
  }

  /** The axis the page shows; the one before it, while a new one is being drawn. Not reactive. */
  let drawn: Axis | undefined;
  /**
   * The day that was held in the middle the last time the axis changed
   * (counted in days from `HELD_FROM`, with its fraction), and where the box
   * was scrolled to for it. Until the reader scrolls, that day is held again:
   * at a far zoom the whole chart may be in sight and the box not scrolled at
   * all, and zooming back in must come back to where the reader was, not to
   * whatever then lay in the middle. Not reactive.
   */
  let held: { day: number; left: number } | undefined;
  const HELD_FROM = calendarDate(2000, 1, 1);

  /*
   * The days in sight stay in sight when the axis changes under them: the
   * zoom, or its first day (a card dated earlier than any before, another
   * "display by", a filter). An effect, because where a box is scrolled to
   * is the page's and not ours: before the page is drawn again the middle
   * of what is in sight is read off the old axis, and once it is drawn the
   * box is scrolled to the same day on the new one.
   */
  $effect.pre(() => {
    const next = axis;
    const before = drawn;
    drawn = next;
    if (!scroller || !before) return;
    if (before.first === next.first && before.dayWidth === next.dayWidth) return;
    const half = daysRoom() / 2;
    const left = scroller.scrollLeft;
    const day =
      held && Math.abs(held.left - left) < 1
        ? held.day
        : daysBetween(HELD_FROM, before.first) + (left + half) / rem() / before.dayWidth;
    void tick().then(() => {
      if (!scroller) return;
      const x = (day - daysBetween(HELD_FROM, next.first)) * next.dayWidth * rem();
      scroller.scrollLeft = Math.max(0, x - half);
      held = { day, left: scroller.scrollLeft };
    });
  });

  /**
   * The box, once it is drawn: it starts with today in sight. The column of
   * cards without a date lies over it: the focus going into the box means
   * the reader has left the column, which closes so that nothing focused is
   * under it. For `{@attach}`.
   */
  function attachScroller(element: HTMLElement): () => void {
    scroller = element;
    untrack(() => {
      showDay(today);
    });
    const left = (): void => {
      if (!sheet) undatedOpen = false;
    };
    element.addEventListener('focusin', left);
    return () => {
      element.removeEventListener('focusin', left);
      scroller = undefined;
    };
  }

  // ------------------------------------------------------------- focus

  /** The bar the Tab key stops on: the one last focused, the first until then. */
  let focused = $state<string | undefined>(undefined);
  const stop = $derived(
    focused !== undefined && chart.bars.has(focused) ? focused : chart.cardLines[0]?.id,
  );

  /**
   * The card whose title opened it. The card gives the focus back to what
   * opened it, and CardDialog would hand it on to the card's bar (`data-card`)
   * and so scroll the days to wherever that bar is: for this card the bar
   * does not say it is the card's, and the focus stays on the title.
   */
  let viaTitle = $state<string | undefined>(undefined);

  /** The title of a row was pressed: the Tab key goes on from this row, and its card opens. */
  function openFromTitle(cardId: string): void {
    focused = cardId;
    viaTitle = cardId;
    onOpenCard(cardId);
  }

  /**
   * A bar took the focus: the stop of the Tab key is on it from now on. The
   * page brings a focused bar into the box by itself, but knows nothing of
   * the column of titles that lies over the box's left: a bar that begins
   * under it is scrolled out from under it. (Room kept at the left by the
   * style sheet, `scroll-padding`, would do it too, and would scroll the
   * days away whenever a title took the focus: the titles lie in that room.)
   */
  function barFocused(cardId: string, bar: HTMLElement): void {
    focused = cardId;
    viaTitle = undefined;
    if (!scroller) return;
    const titles = scroller.querySelector('[data-gantt-corner]')?.getBoundingClientRect().right;
    const hidden = (titles ?? 0) - bar.getBoundingClientRect().left;
    if (hidden > 0) scroller.scrollLeft -= hidden + rem() / 2;
  }

  function focusBar(cardId: string | undefined): void {
    if (cardId === undefined) return;
    focused = cardId;
    scroller?.querySelector<HTMLElement>(`[data-bar="${CSS.escape(cardId)}"]`)?.focus();
  }

  /** Goes to the next of `cardIds` after the bar last focused, round and round. */
  function focusNext(cardIds: readonly string[]): void {
    const at = focused === undefined ? -1 : cardIds.indexOf(focused);
    focusBar(cardIds[(at + 1) % cardIds.length]);
  }

  /** The arrow keys walk the bars: up and down a row, Home and End to the first and last. */
  function barKeys(event: KeyboardEvent, cardId: string): void {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const lines = chart.cardLines;
    const at = lines.findIndex((line) => line.id === cardId);
    const to = { ArrowUp: at - 1, ArrowDown: at + 1, Home: 0, End: lines.length - 1 }[event.key];
    if (to === undefined) return;
    event.preventDefault();
    focusBar(lines[Math.min(lines.length - 1, Math.max(0, to))]?.id);
  }

  // ------------------------------------------------ cards without a date

  const sheet = $derived(overlayPresentation(page.stage) === 'sheet');
  /**
   * The cards without a date are open: a column at the right, or a sheet on
   * a compact page. Closed when the page changes between the two, so that a
   * device turned on its side does not raise a sheet unasked.
   */
  let undatedOpen = $derived<boolean>(sheet && false);
  /** "No date" in the row above, and the column it opens: where the focus goes and comes back. */
  let undatedButton = $state<HTMLElement | null>(null);
  let undatedPanel: HTMLElement | undefined;

  const undatedRows = $derived({
    placed: chart.placed,
    templates: board.board?.cardProperties ?? [],
    visiblePropertyIds: view.fields.visiblePropertyIds,
    users: board.workspace.users,
    nameFormat: board.workspace.nameFormat,
    cardHref,
  });

  async function toggleUndated(): Promise<void> {
    undatedOpen = !undatedOpen;
    if (!undatedOpen) return;
    await tick();
    undatedPanel?.querySelector<HTMLElement>('[data-testid="undated-close"]')?.focus();
  }

  /** Closes the column of cards without a date, the focus back on its button. */
  function closeUndated(): void {
    undatedOpen = false;
    undatedButton?.focus();
  }

  /** The column: kept for the focus, and Escape in it closes it. For `{@attach}`. */
  function attachUndated(element: HTMLElement): () => void {
    undatedPanel = element;
    const keys = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') closeUndated();
    };
    element.addEventListener('keydown', keys);
    return () => {
      element.removeEventListener('keydown', keys);
      undatedPanel = undefined;
    };
  }

  /**
   * A card opened from the cards without a date. Given a date, it leaves the
   * list: the focus is put on "No date" first, which is where the card gives
   * it back then (CardDialog gives it to the card's bar where there is one).
   * On a sheet, the sheet closes first.
   */
  async function openUndated(cardId: string): Promise<void> {
    if (sheet) {
      undatedOpen = false;
      await tick();
    }
    undatedButton?.focus();
    onOpenCard(cardId);
  }
</script>

<div
  data-testid="gantt"
  data-zoom={zoom}
  data-first={axis.first}
  data-last={axis.last}
  data-edges={chart.relations.edges.length}
  data-late={chart.relations.late.size}
  class="flex h-full min-h-0 flex-col [--gantt-row:2.25rem] [--gantt-title:7rem] @columns/pane:[--gantt-title:14rem] roomy:[--gantt-row:2.75rem]"
  style:--gantt-width="{axis.width}rem"
>
  <Toolbar
    ontoday={() => {
      showDay(today);
    }}
    late={chart.relations.late.size}
    onlate={() => {
      focusNext(chart.lateIds);
    }}
    loops={chart.loopIds.length}
    onloop={() => {
      focusNext(chart.loopIds);
    }}
    {zoom}
    onzoom={setZoom}
    undated={chart.placed.undated.length}
    {undatedOpen}
    bind:undatedButton
    onundated={() => void toggleUndated()}
  />

  <!-- Measured here, outside the scrollbars of the box inside: the width does not depend on them. -->
  <div class="relative flex min-h-0 flex-1 flex-col" {@attach box.attach}>
    {#if box.measured}
      <div
        data-testid="gantt-scroller"
        class="min-h-0 flex-1 scroll-pt-12 overflow-auto overscroll-contain"
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
            <!--
              `role`: a list without bullets is not announced as one everywhere.
              The rows stand in blocks, and a block that is out of view is not
              laid out or painted until it comes near (`content-visibility`).
              Blocks and not single rows: the page looks at every such box on
              every frame of scrolling, and thousands of them halve the frames.
            -->
            <ul role="list" aria-label={m.gantt_rows()} class="relative">
              {#each chart.blocks as block, at (at)}
                <div
                  role="presentation"
                  class="[contain-intrinsic-block-size:auto_calc(var(--gantt-row)*var(--rows))] [content-visibility:auto]"
                  style:--rows={block.length}
                >
                  {#each block as line (`${line.kind}:${line.id}`)}
                    {#if line.kind === 'group'}
                      <GroupRow {line} />
                    {:else}
                      <BarRow
                        {line}
                        current={line.id === stop}
                        href={cardHref(line.id)}
                        viaTitle={line.id === viaTitle}
                        onopen={onOpenCard}
                        onopentitle={openFromTitle}
                        onkeys={barKeys}
                        onfocused={barFocused}
                      />
                    {/if}
                  {/each}
                </div>
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

    {#if undatedOpen && !sheet}
      <div
        data-testid="undated-panel"
        class="absolute inset-y-0 right-0 z-40 flex w-80 max-w-full flex-col border-l bg-background pb-(--safe-bottom) shadow-lg"
        {@attach attachUndated}
      >
        <Undated
          {...undatedRows}
          onOpenCard={(cardId: string) => void openUndated(cardId)}
          onclose={closeUndated}
        />
      </div>
    {/if}
  </div>
</div>

{#if sheet}
  <BottomSheet
    bind:open={undatedOpen}
    title={m.calendar_undated_count({ count: chart.placed.undated.length })}
  >
    <Undated {...undatedRows} onOpenCard={(cardId: string) => void openUndated(cardId)} />
  </BottomSheet>
{/if}
