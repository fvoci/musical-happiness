<!--
  The calendar view: the cards of the view placed on the days of a month by
  one date property (`displayProperty`: the one the view names, or else the
  first kind of date of the board, or else when the card was made).

  Above, a row to go back and on a month and to today. Under it, a box whose
  width decides what is drawn (docs/calendar-proposal.md):

  - 34rem and wider: the weeks of the month as a grid of bars. A day opens
    its cards in a popover next to its cell, or on a sheet on a compact
    page (questions C2, C3).
  - narrower: a small grid of numbers, dots and lines over the list of the
    picked day (question C1, answer A).

  The month in view and the day picked are kept here, above the parts that
  draw them: a window that changes size swaps the grid of bars for the small
  grid, and the two must not take the month and the day with them. They are
  the state of the screen only; another view, or a new load, starts at
  today (question C19).
-->
<script lang="ts">
  import { tick, untrack } from 'svelte';

  import BottomSheet from '$lib/components/sheet/BottomSheet.svelte';
  import { overlayPresentation } from '$lib/layout/presentation.ts';
  import { fingerPointer, insets, page, PaneRoom } from '$lib/layout/room.svelte.ts';
  import {
    displayProperty,
    firstOf,
    inMonth,
    monthOf,
    shiftMonth,
    weeksOfMonth,
  } from '$lib/logic/calendar.ts';
  import { addDays, dateOfInstant, type CalendarDate } from '$lib/logic/dates.ts';
  import { displayDayLong, displayMonth } from '$lib/logic/format.ts';
  import * as Popover from '$lib/ui/popover/index.js';

  import type { ViewProps } from '../props.ts';

  import DayList from './DayList.svelte';
  import MiniMonth from './MiniMonth.svelte';
  import MonthGrid from './MonthGrid.svelte';
  import { Placed } from './placed.ts';
  import Toolbar from './Toolbar.svelte';

  let { board, view, cards, cardHref, onOpenCard }: ViewProps = $props();

  const locale = $derived(board.workspace.locale);
  const regional = $derived(board.workspace.regional);
  const templates = $derived(board.board?.cardProperties ?? []);
  const template = $derived(displayProperty(view.fields.dateDisplayPropertyId, templates));
  /** Today, on the reader's calendar. */
  const today = $derived(dateOfInstant(Date.now(), regional.timeZone));

  let month = $state(monthOf(untrack(() => today)));
  let selected = $state(untrack(() => today));
  /** The cards of the picked day are open over the grid of bars. */
  let dayOpen = $state(false);

  const weeks = $derived(weeksOfMonth(month, regional.weekStart));
  const placed = $derived(new Placed(cards, template, regional, locale, weeks.flat()));

  /** The box the calendar is drawn in: its width decides between bars and the small grid. */
  const box = new PaneRoom();
  const bars = $derived(box.columns);
  const finger = $derived(fingerPointer.current);
  const sheet = $derived(overlayPresentation(page.stage) === 'sheet');

  /** The box, for finding the button of a day in it. Not reactive: read on a key press. */
  let body: HTMLElement | undefined;

  /** The box: kept for the focus, and measured. For `{@attach}`. */
  function attachBody(element: HTMLElement): () => void {
    body = element;
    return box.attach(element);
  }

  /** Puts the focus on the button of the picked day. */
  function focusDay(): void {
    body
      ?.querySelector<HTMLElement>(
        `[data-day="${selected}"][data-day-button], [data-day="${selected}"] [data-day-button]`,
      )
      ?.focus();
  }

  /** Picks a day; a day of another month brings that month into view (question C21). */
  function select(date: CalendarDate): void {
    selected = date;
    if (!inMonth(date, month)) month = monthOf(date);
  }

  /** One month on or back. The picked day is today in the month of today, else the first. */
  function step(by: number): void {
    dayOpen = false;
    month = shiftMonth(month, by);
    selected = inMonth(today, month) ? today : firstOf(month);
  }

  function goToday(): void {
    dayOpen = false;
    month = monthOf(today);
    selected = today;
  }

  function pick(date: CalendarDate): void {
    select(date);
    if (bars) dayOpen = true;
  }

  /** An arrow key: the picked day moves, and the focus with it. */
  async function move(by: number): Promise<void> {
    select(addDays(selected, by));
    await tick();
    focusDay();
  }

  const lists = $derived({
    placed,
    today,
    templates,
    visiblePropertyIds: view.fields.visiblePropertyIds,
    users: board.workspace.users,
    nameFormat: board.workspace.nameFormat,
    cardHref,
  });

  /** A card opened from the list of a day: the list closes behind it. */
  function openFromList(cardId: string): void {
    dayOpen = false;
    onOpenCard(cardId);
  }
</script>

<div
  data-testid="calendar"
  data-month="{month.year}-{String(month.month).padStart(2, '0')}"
  data-selected={selected}
  data-layout={box.measured ? (bars ? 'bars' : 'mini') : undefined}
  class="flex h-full min-h-0 flex-col"
>
  <Toolbar title={displayMonth(firstOf(month), locale)} onstep={step} ontoday={goToday} />

  <div class="relative flex min-h-0 flex-1 flex-col" {@attach attachBody}>
    {#if !box.measured}
      <!-- Measured first: what is drawn depends on the width. -->
    {:else if bars}
      <MonthGrid
        {weeks}
        {placed}
        {month}
        {today}
        {selected}
        {finger}
        {cardHref}
        {onOpenCard}
        onpick={pick}
        onmove={(by: number) => void move(by)}
      />
    {:else}
      <MiniMonth
        {weeks}
        {placed}
        {month}
        {today}
        {selected}
        onpick={select}
        onmove={(by: number) => void move(by)}
      />
      <div class="flex min-h-0 flex-1 flex-col border-t pb-(--safe-bottom)">
        <DayList date={selected} {...lists} {onOpenCard} />
      </div>
    {/if}
  </div>
</div>

{#if bars && sheet}
  <BottomSheet bind:open={dayOpen} title={displayDayLong(selected, locale)}>
    <DayList date={selected} {...lists} onOpenCard={openFromList} />
  </BottomSheet>
{:else if bars}
  <Popover.Root bind:open={dayOpen}>
    <Popover.Content
      customAnchor={`[data-testid="month-grid"] [data-day="${selected}"]`}
      side="right"
      align="start"
      collisionPadding={insets.collisionPadding}
      aria-label={displayDayLong(selected, locale)}
      data-testid="day-popover"
      class="max-h-[min(30rem,calc(100dvh-1rem))] w-80 max-w-[calc(100vw-1rem-var(--safe-left)-var(--safe-right))] gap-0 p-0 pb-1"
      onOpenAutoFocus={(event: Event) => {
        // A finger has not asked for the keyboard, and its ring is not wanted either.
        if (finger) event.preventDefault();
      }}
      onCloseAutoFocus={(event: Event) => {
        // No trigger to go back to: the focus goes to the day the list was of.
        event.preventDefault();
        focusDay();
      }}
    >
      <DayList
        date={selected}
        {...lists}
        onOpenCard={openFromList}
        onclose={() => {
          dayOpen = false;
        }}
      />
    </Popover.Content>
  </Popover.Root>
{/if}
