<!--
  The calendar view: the cards of the view placed on the days of a month or
  of a week (question C17) by one date property (`displayProperty`: the one the view names, or else the
  first kind of date of the board, or else when the card was made).

  Above, a row to go back and on a month or a week, to today, and between
  month and week. Under it, a box whose width decides what is drawn
  (docs/calendar-proposal.md):

  - 34rem and wider: the weeks of the month as a grid of bars, or one week
    as high as the box with every card in it. A day opens its cards in a
    popover next to its cell, or on a sheet on a compact page (C2, C3).
  - narrower: a small grid of numbers, dots and lines over the list of the
    picked day (C1, answer A), or the strip of a week over the lists of its
    seven days (WeekAgenda).

  A card is made on a day from "New card on …" at the end of the day's list,
  or with a mouse from the "+" of its cell (question C11, answer B).
  A card's date is changed from the "…" of its line in a list: a picker of
  days (DayPicker) moves it whole (questions C9, C10). Where the cards are
  placed by a date nobody sets, no line offers it and the lists say why.

  Month or week, the month in view and the day picked are kept above the
  parts that draw them (Period): a window that changes size swaps the grid of
  bars for the small grid, and must not take them along. Month or week is
  remembered on the device for each view; the month and the day are the
  screen's only, and a new load starts at today (question C19).
-->
<script lang="ts">
  import { tick, untrack } from 'svelte';

  import BottomSheet from '$lib/components/sheet/BottomSheet.svelte';
  import { overlayPresentation } from '$lib/layout/presentation.ts';
  import { fingerPointer, page, PaneRoom } from '$lib/layout/room.svelte.ts';
  import { datesWritable, displayProperty } from '$lib/logic/calendar.ts';
  import { addDays, dateOfInstant, type CalendarDate } from '$lib/logic/dates.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';

  import type { ViewProps } from '../props.ts';

  import DayList from './DayList.svelte';
  import DayOverlay from './DayOverlay.svelte';
  import DayPicker from './DayPicker.svelte';
  import { CardMaker } from './making.svelte.ts';
  import MiniMonth from './MiniMonth.svelte';
  import MonthGrid from './MonthGrid.svelte';
  import { periodTitle } from './period-title.ts';
  import { Period, type Scale } from './period.svelte.ts';
  import { MinuteClock } from './clock.svelte.ts';
  import { Placed } from './placed.ts';
  import Toolbar from './Toolbar.svelte';
  import Undated from './Undated.svelte';
  import WeekAgenda from './WeekAgenda.svelte';

  let { board, view, cards, cardHref, onOpenCard }: ViewProps = $props();

  const locale = $derived(board.workspace.locale);
  const regional = $derived(board.workspace.regional);
  const templates = $derived(board.board?.cardProperties ?? []);
  const template = $derived(displayProperty(view.fields.dateDisplayPropertyId, templates));
  const clock = new MinuteClock();
  /** Today, on the reader's calendar. */
  const today = $derived(dateOfInstant(clock.now, regional.timeZone));

  const period = new Period(
    untrack(() => view.id),
    untrack(() => today),
  );
  const month = $derived(period.month);
  const selected = $derived(period.selected);
  const scale = $derived(period.scale);
  /** The box is wide enough for bars. */
  const box = new PaneRoom();
  const bars = $derived(box.columns);
  const sheet = $derived(overlayPresentation(page.stage) === 'sheet');
  /**
   * The cards of the picked day are open over the grid of bars. Closed
   * whenever the box goes from bars to the small grid or back, so that a box
   * that widens again does not bring them back unasked.
   */
  let dayOpen = $derived<boolean>(bars && false);
  /**
   * The cards without a date are open: a column at the right, or a sheet on a
   * compact page. Closed when the page changes between the two, so that a
   * device turned on its side does not raise a sheet unasked.
   */
  let undatedOpen = $derived<boolean>(sheet && false);

  const weeks = $derived(period.weeks(regional.weekStart));
  const placed = $derived(new Placed(cards, template, regional, locale, weeks.flat()));

  const finger = $derived(fingerPointer.current);

  /** The box, for finding the button of a day in it. Not reactive: read on a key press. */
  let body: HTMLElement | undefined;
  /** "No date" in the row above, and the column it opens: where the focus goes and comes back. */
  let undatedButton = $state<HTMLElement | null>(null);
  let undatedPanel: HTMLElement | undefined;

  /**
   * The box: kept for the focus, and measured. The column of cards without a
   * date lies over it: the focus going into the box (Shift+Tab out of the
   * column, an arrow key) means the reader has left the column, which closes
   * so that nothing focused is under it. For `{@attach}`.
   */
  function attachBody(element: HTMLElement): () => void {
    body = element;
    const left = (): void => {
      if (!sheet) undatedOpen = false;
    };
    element.addEventListener('focusin', left);
    return () => {
      element.removeEventListener('focusin', left);
    };
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
    period.select(date);
  }

  /** A month or a week on or back. */
  function step(by: number): void {
    dayOpen = false;
    period.step(by, today);
  }

  function goToday(): void {
    dayOpen = false;
    period.goTo(today);
  }

  function pick(date: CalendarDate): void {
    undatedOpen = false;
    select(date);
    if (bars) dayOpen = true;
  }

  /** An arrow key: the picked day moves, and the focus with it. */
  async function move(by: number): Promise<void> {
    select(addDays(selected, by));
    await tick();
    focusDay();
  }

  const writable = $derived(datesWritable(template));
  const rows = $derived({
    placed,
    templates,
    visiblePropertyIds: view.fields.visiblePropertyIds,
    users: board.workspace.users,
    nameFormat: board.workspace.nameFormat,
    cardHref,
    board,
    onChangeDate: writable ? (cardId: string) => void changeDate(cardId) : undefined,
  });
  const fixed = $derived(
    writable
      ? undefined
      : m.calendar_dates_fixed({
          property: template?.name ?? m['PropertyType.CreatedTime'](),
        }),
  );
  const lists = $derived({ ...rows, today, fixed });

  /** Opens or closes the cards without a date; the list of a day gives way to them. */
  async function toggleUndated(): Promise<void> {
    undatedOpen = !undatedOpen;
    if (!undatedOpen) return;
    dayOpen = false;
    await tick();
    undatedPanel?.querySelector<HTMLElement>('[data-testid="undated-close"]')?.focus();
  }

  /**
   * The column of cards without a date: kept for the focus, and Escape in it
   * closes it. A listener and not `onkeydown`: the column is a region of the
   * page, not a control, and the key is the column's way out like its close
   * button. For `{@attach}`.
   */
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

  /** Closes the column of cards without a date, the focus back on its button. */
  function closeUndated(): void {
    undatedOpen = false;
    undatedButton?.focus();
  }

  /**
   * A card opened from the cards without a date. Given a date, it may leave
   * the list and have no bar in view either: the focus is put on "No date"
   * first, which is where the card gives it back then (CardDialog gives it to
   * the card's bar where there is one). On a sheet, the sheet closes first.
   */
  async function openUndated(cardId: string): Promise<void> {
    if (sheet) {
      undatedOpen = false;
      await tick();
    }
    undatedButton?.focus();
    onOpenCard(cardId);
  }

  // -------------------------------------------------------- making a card

  /** Cards can be made on a day: by someone who may edit, where the day is a date to set. */
  const creatable = $derived(writable && board.canEdit);

  const maker = new CardMaker(() => ({ board, view, templates, template }), {
    select,
    focusDay,
    onOpenCard: (cardId: string) => {
      onOpenCard(cardId);
    },
  });
  const createOn = (date: CalendarDate): void => {
    void maker.on(date);
  };

  // ------------------------------------------------------ changing a date

  /** The card whose date the picker of a day changes, and whether the picker is open. */
  let changing = $state<string | undefined>(undefined);
  let pickerOpen = $state(false);

  /** "Change date" in a card's menu: the lists close, and the picker opens by the picked day. */
  async function changeDate(cardId: string): Promise<void> {
    dayOpen = false;
    undatedOpen = false;
    changing = cardId;
    await tick();
    pickerOpen = true;
  }

  /**
   * The board kept a card's move: the calendar goes to its day, and the
   * focus with it if it is on the day the card was on.
   */
  async function moved(date: CalendarDate): Promise<void> {
    const onDay = body?.contains(document.activeElement) ?? false;
    select(date);
    await tick();
    if (onDay) focusDay();
  }

  /** The picker has closed: the focus goes to the card's day, or back to "No date". */
  async function pickerClosed(dated: boolean): Promise<void> {
    await tick();
    if (dated) focusDay();
    else undatedButton?.focus();
  }
</script>

<div
  data-testid="calendar"
  data-month="{month.year}-{String(month.month).padStart(2, '0')}"
  data-selected={selected}
  data-scale={scale}
  data-layout={box.measured ? (bars ? 'bars' : 'mini') : undefined}
  class="flex h-full min-h-0 flex-col"
>
  <Toolbar
    title={periodTitle(scale, weeks, month, locale)}
    {scale}
    onscale={(next: Scale) => {
      dayOpen = false;
      period.scale = next;
    }}
    onstep={step}
    ontoday={goToday}
    undated={placed.undated.length}
    {undatedOpen}
    bind:undatedButton
    onundated={() => void toggleUndated()}
  />

  <!-- Measured here, outside the scrollbar a low window gives the box: the width does not depend on it. -->
  <div class="relative flex min-h-0 flex-1 flex-col" {@attach box.attach}>
    <!-- A low window scrolls the small grid and its list together; the list keeps room for two cards. -->
    <div
      class={['relative flex min-h-0 flex-1 flex-col', !bars && 'overflow-y-auto']}
      {@attach attachBody}
    >
      {#if !box.measured}
        <!-- Measured first: what is drawn depends on the width. -->
      {:else if bars}
        <MonthGrid
          {weeks}
          week={scale === 'week'}
          {placed}
          {month}
          {today}
          {selected}
          {finger}
          {cardHref}
          {onOpenCard}
          onpick={pick}
          onmove={(by: number) => void move(by)}
          oncreate={creatable ? createOn : undefined}
        />
      {:else if scale === 'week'}
        <div
          class={[
            'flex min-h-0 flex-1 flex-col',
            board.canEdit && 'pb-[calc(var(--safe-bottom)+5rem)]',
          ]}
        >
          <WeekAgenda
            week={weeks[0] ?? []}
            {placed}
            {today}
            {selected}
            rows={{ ...rows, onOpenCard }}
            onpick={pick}
            onmove={(by: number) => void move(by)}
            oncreate={creatable ? createOn : undefined}
          />
        </div>
      {:else}
        <MiniMonth
          {weeks}
          {placed}
          {month}
          {today}
          {selected}
          onpick={pick}
          onmove={(by: number) => void move(by)}
        />
        <!-- Room at the bottom for the round "New" of a folded header, which would cover the last line. -->
        <div
          class={[
            'flex min-h-36 flex-1 flex-col border-t',
            board.canEdit ? 'pb-[calc(var(--safe-bottom)+5rem)]' : 'pb-(--safe-bottom)',
          ]}
        >
          <DayList
            date={selected}
            {...lists}
            {onOpenCard}
            oncreate={creatable ? createOn : undefined}
          />
        </div>
      {/if}
    </div>

    {#if undatedOpen && !sheet}
      <!--
        Over the grid, at its right: the proposal's column stands beside the
        grid only from a pane of 54rem, a width this step does not add.
      -->
      <div
        data-testid="undated-panel"
        class="absolute inset-y-0 right-0 z-30 flex w-80 max-w-full flex-col border-l bg-background pb-(--safe-bottom) shadow-lg"
        {@attach attachUndated}
      >
        <Undated
          {...rows}
          onOpenCard={(cardId: string) => void openUndated(cardId)}
          onclose={closeUndated}
        />
      </div>
    {/if}
  </div>
</div>

<DayPicker
  bind:cardId={changing}
  bind:open={pickerOpen}
  {board}
  {placed}
  {template}
  {selected}
  {today}
  {sheet}
  onmoved={(date: CalendarDate) => void moved(date)}
  onclosed={(dated: boolean) => void pickerClosed(dated)}
/>

{#if sheet}
  <BottomSheet
    bind:open={undatedOpen}
    title={m.calendar_undated_count({ count: placed.undated.length })}
  >
    <Undated {...rows} onOpenCard={(cardId: string) => void openUndated(cardId)} />
  </BottomSheet>
{/if}

{#if bars}
  <DayOverlay
    bind:open={dayOpen}
    {sheet}
    {finger}
    {selected}
    {locale}
    {lists}
    mayFocusDay={() => !undatedOpen}
    {focusDay}
    {onOpenCard}
    oncreate={creatable ? createOn : undefined}
  />
{/if}
