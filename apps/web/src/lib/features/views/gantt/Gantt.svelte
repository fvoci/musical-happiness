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

  Where the reader may change the dates (a date property they may edit), a
  bar is dragged to other days and its ends to make it longer or shorter
  (bar-drag.svelte.ts); a press without a drag opens the card. With the
  keys, "Change date" in the card's "…" at the end of its title moves it
  (a picker of days, as on the calendar: DayPicker). The header's "New"
  makes a card of one day on today, or on the first day in sight.

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
  the view and kept while the view is shown where not. Where the chart is scrolled to
  is the state of the screen only: another view, or a new load, starts with
  today in sight.
-->
<script lang="ts">
  import { tick, untrack } from 'svelte';

  import BottomSheet from '$lib/components/sheet/BottomSheet.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { overlayPresentation } from '$lib/layout/presentation.ts';
  import { page, Room } from '$lib/layout/room.svelte.ts';
  import { datesWritable, newCardProperties } from '$lib/logic/calendar.ts';
  import { addDays, dateOfInstant, type CalendarDate } from '$lib/logic/dates.ts';
  import { barBox, DAY_WIDTH, xOfDay, zoomOf, type Axis } from '$lib/logic/gantt.ts';
  import type { Days } from '$lib/logic/dependencies.ts';
  import type { GanttZoom } from '$lib/logic/view-members.ts';
  import { registerNewCard } from '$lib/state/index.ts';

  import { MinuteClock } from '../calendar/clock.svelte.ts';
  import CardMenu from '../calendar/CardMenu.svelte';
  import DayPicker from '../calendar/DayPicker.svelte';
  import Undated from '../calendar/Undated.svelte';
  import { writeDays } from '../calendar/write.ts';
  import { UndatedColumn } from '../calendar/undated-column.svelte.ts';
  import type { ViewProps } from '../props.ts';

  import Arrows from './Arrows.svelte';
  import AxisHeader from './AxisHeader.svelte';
  import { BarDrag, type BarGrip } from './bar-drag.svelte.ts';
  import { BarFocus } from './bar-focus.svelte.ts';
  import BarRow from './BarRow.svelte';
  import { GanttChart } from './chart.svelte.ts';
  import { DaysHold } from './days-hold.svelte.ts';
  import GroupRow from './GroupRow.svelte';
  import { daysLabel } from './names.ts';
  import Toolbar from './Toolbar.svelte';

  let { board, view, cards, cardHref, onOpenCard }: ViewProps = $props();

  const locale = $derived(board.workspace.locale);
  const clock = new MinuteClock();
  /** Today, on the reader's calendar. */
  const today = $derived(dateOfInstant(clock.now, board.workspace.regional.timeZone));
  /** The zoom a reader who may not change the view chose: theirs until another view is shown. */
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

  const drag: BarDrag = new BarDrag({
    axis: (): Axis => chart.axis,
    days: (cardId: string) => chart.placed.span(cardId),
    enabled: () => writable,
    scroller: () => scroller,
    titles,
    rem,
    ondrop: dropBar,
  });

  const chart: GanttChart = new GanttChart({
    board: () => board,
    view: () => view,
    cards: () => cards,
    today: () => today,
    zoom: () => zoom,
    minDays: () => minDays,
    reach: (): Days | undefined => drag.reach,
  });

  /** Bars can be dragged: by someone who may edit, where they are placed by a date to set. */
  const writable = $derived(board.canEdit && datesWritable(chart.template));

  /** A bar let go on other days: its card's date is written, and the board shows it at once. */
  function dropBar(cardId: string, grip: BarGrip, days: Days): void {
    const card = chart.placed.card(cardId);
    const template = chart.template;
    if (!card || !template || !datesWritable(template)) return;
    void writeDays(board, card, template, grip, days, board.workspace.regional.timeZone);
  }

  /** The bar being dragged as it is drawn meanwhile. */
  const dragged = $derived.by(() => {
    const preview = drag.preview;
    if (!preview) return undefined;
    const days = daysLabel(preview.days, locale, Number(today.slice(0, 4)), (first, last) =>
      m.gantt_days_range({ first, last }),
    );
    return { id: preview.id, box: barBox(axis, preview.days), days };
  });

  const axis = $derived(chart.axis);
  /** The middle of today, in rem from the start of the axis. */
  const todayX = $derived(xOfDay(axis, today) + axis.dayWidth / 2);

  /** The box that scrolls. Not reactive: read when something is brought into view. */
  let scroller: HTMLElement | undefined;

  /** How wide the column of titles is, in pixels. */
  function titles(): number {
    return scroller?.querySelector<HTMLElement>('[data-gantt-corner]')?.offsetWidth ?? 0;
  }

  /** How wide the days in sight are, in pixels: the box less the column of titles. */
  function daysRoom(): number {
    return scroller ? scroller.clientWidth - titles() : 0;
  }

  /** Scrolls the days so that the middle of `date` is `part` of the way into what is in sight. */
  function showDay(date: CalendarDate, part = 1 / 3): void {
    if (!scroller) return;
    const x = (xOfDay(axis, date) + axis.dayWidth / 2) * rem();
    scroller.scrollLeft = Math.max(0, x - daysRoom() * part);
    hold.release();
  }

  /** Another zoom: the view's where the reader may change the view, theirs for the visit where not. */
  function setZoom(next: GanttZoom): void {
    if (next === zoom) return;
    if (board.canEdit) void board.setViewFields(view.id, { ganttZoom: next }, 'ganttZoom');
    else ownZoom = next;
  }

  /** The days in sight stay in sight when the zoom or the start of the axis changes. */
  const hold = new DaysHold({
    axis: () => axis,
    today: () => today,
    scroller: () => scroller,
    room: daysRoom,
    rem,
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
      undated.left();
    };
    element.addEventListener('focusin', left);
    const detach = drag.attach(element);
    return () => {
      detach();
      element.removeEventListener('focusin', left);
      scroller = undefined;
    };
  }

  // ------------------------------------------------------------- focus

  const focus = new BarFocus({
    chart: () => chart,
    scroller: () => scroller,
    undatedButton: () => undated.button,
    onOpenCard: (cardId: string) => {
      onOpenCard(cardId);
    },
    rem,
  });

  // ------------------------------------------------------- making a card

  /** A card is being made: a second "New" meanwhile makes nothing. Not reactive. */
  let making = false;

  /** The first and the last day wholly in sight. */
  function daysInSight(): { first: CalendarDate; last: CalendarDate } {
    const day = axis.dayWidth * rem();
    const left = scroller?.scrollLeft ?? 0;
    const first = Math.min(axis.days - 1, Math.ceil(left / day));
    const last = Math.max(first, Math.floor((left + daysRoom()) / day) - 1);
    return { first: addDays(axis.first, first), last: addDays(axis.first, last) };
  }

  /**
   * The header's "New" (question C13): a card of one day, today, or the
   * first day in sight where today is not; with the values the view's
   * filter asks for. It opens, as a card made on a board does.
   */
  async function makeCard(): Promise<void> {
    if (making) return;
    making = true;
    try {
      const sight = daysInSight();
      const day = today >= sight.first && today <= sight.last ? today : sight.first;
      const templates = board.board?.cardProperties ?? [];
      const properties = newCardProperties(view.fields.filter, templates, chart.template, day);
      const card = await board.createCard({ properties });
      if (card) focus.openCard(card.id);
    } finally {
      making = false;
    }
  }

  $effect(() => {
    if (!writable) return;
    return registerNewCard(view.id, makeCard);
  });

  // ------------------------------------------------------ changing a date

  /** The card whose date the picker of days changes, the picker, and the row it hangs by. */
  let changing = $state<string | undefined>(undefined);
  let pickerOpen = $state(false);
  let changingRow = $state<HTMLElement | null>(null);
  /** The card last named to the picker: the picker forgets it as it closes. Not reactive. */
  let changed: string | undefined;

  /** "Change date" in a card's "…": the picker opens by the card's row. */
  async function changeDate(cardId: string): Promise<void> {
    changingRow =
      scroller?.querySelector<HTMLElement>(`[data-row="${CSS.escape(cardId)}"]`) ?? null;
    undated.open = false;
    changing = cardId;
    changed = cardId;
    await tick();
    pickerOpen = true;
  }

  /** The picker has closed: the focus goes back to the card's bar, or to "No date". */
  async function pickerClosed(): Promise<void> {
    await tick();
    const cardId = changed;
    const row = cardId
      ? scroller?.querySelector<HTMLElement>(`[data-row="${CSS.escape(cardId)}"]`)
      : null;
    changingRow = null;
    const back = row?.querySelector<HTMLElement>('[data-bar]');
    if (back) back.focus();
    else undated.button?.focus();
  }

  /** The board kept a move to `date`: the day comes into sight, if it is not. */
  function moved(date: CalendarDate): void {
    if (!scroller) return;
    const x = (xOfDay(axis, date) + axis.dayWidth / 2) * rem() - scroller.scrollLeft;
    if (x < 0 || x > daysRoom()) showDay(date);
  }

  // ------------------------------------------------ cards without a date

  const sheet = $derived(overlayPresentation(page.stage) === 'sheet');
  const undated = new UndatedColumn({
    sheet: () => sheet,
    onOpenCard: (cardId: string) => {
      onOpenCard(cardId);
    },
  });

  const undatedRows = $derived({
    placed: chart.placed,
    templates: board.board?.cardProperties ?? [],
    visiblePropertyIds: view.fields.visiblePropertyIds,
    users: board.workspace.users,
    board,
    cardHref,
    hint: (property: string) => m.gantt_undated_hint({ property }),
    loops: chart.undatedLoops,
  });
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
    related={chart.dependency !== undefined}
    late={chart.relations.late.size}
    onlate={() => {
      focus.focusNext('late');
    }}
    loops={chart.loopCount > 0 ? chart.loopCount : chart.undatedLoops.size}
    onloop={() => {
      // Cards without a date have no bar: their list shows which are on a loop.
      if (chart.loopCount > 0) focus.focusNext('loop');
      else if (!undated.open) void undated.toggle();
    }}
    {zoom}
    onzoom={setZoom}
    undated={chart.placed.undated.length}
    undatedCard={focus.openedUndated}
    undatedOpen={undated.open}
    bind:undatedButton={undated.button}
    onundated={() => void undated.toggle()}
  />

  <!-- Measured here, outside the scrollbars of the box inside: the width does not depend on them. -->
  <div class="relative flex min-h-0 flex-1 flex-col" {@attach box.attach}>
    {#if box.measured}
      <div
        data-testid="gantt-scroller"
        tabindex="-1"
        class="min-h-0 flex-1 scroll-pt-12 overflow-auto overscroll-contain outline-none"
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
              A skipped block is as high as its rows, worked out and not
              remembered from when it was last drawn: rows change their height
              with the reader's density, and a block its number of rows.
            -->
            <ul role="list" aria-label={m.gantt_rows()} class="relative">
              {#each chart.blocks as block, at (at)}
                <div
                  role="presentation"
                  class="[contain-intrinsic-block-size:calc(var(--gantt-row)*var(--rows))] [content-visibility:auto]"
                  style:--rows={block.length}
                >
                  {#each block as line (`${line.kind}:${line.id}`)}
                    {#if line.kind === 'group'}
                      <GroupRow {line} />
                    {:else}
                      <BarRow
                        {line}
                        current={line.id === focus.stop}
                        href={cardHref(line.id)}
                        viaTitle={line.id === focus.viaTitle}
                        pointed={line.id === focus.pointed}
                        onopen={focus.openCard}
                        onopentitle={focus.openFromTitle}
                        onkeys={focus.barKeys}
                        onfocused={focus.barFocused}
                        draggable={writable}
                        menu={board.canEdit ? rowMenu : undefined}
                        dragged={dragged?.id === line.id ? dragged : undefined}
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

    {#if undated.open && !sheet}
      <div
        data-testid="undated-panel"
        class="absolute inset-y-0 right-0 z-40 flex w-80 max-w-full flex-col border-l bg-background pb-(--safe-bottom) shadow-lg"
        {@attach undated.attach}
      >
        <Undated
          {...undatedRows}
          onOpenCard={(cardId: string) => void undated.openCard(cardId)}
          onclose={undated.close}
        />
      </div>
    {/if}
  </div>
</div>

{#snippet rowMenu(cardId: string)}
  {@const card = chart.placed.card(cardId)}
  {#if card}
    <CardMenu
      {board}
      {card}
      title={chart.placed.title(card)}
      dated
      quiet
      onChangeDate={writable ? (id: string) => void changeDate(id) : undefined}
    />
  {/if}
{/snippet}

<DayPicker
  bind:cardId={changing}
  bind:open={pickerOpen}
  line={changingRow}
  {board}
  placed={chart.placed}
  template={chart.template}
  selected={changing ? (chart.placed.span(changing)?.first ?? today) : today}
  {today}
  {sheet}
  onmoved={moved}
  onclosed={() => void pickerClosed()}
/>

{#if sheet}
  <BottomSheet
    bind:open={undated.open}
    title={m.calendar_undated_count({ count: chart.placed.undated.length })}
  >
    <Undated {...undatedRows} onOpenCard={(cardId: string) => void undated.openCard(cardId)} />
  </BottomSheet>
{/if}
