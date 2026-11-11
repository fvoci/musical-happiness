<!--
  "Change date" of a card on the calendar: a month of days to pick its new
  first day from (question C9, answer B). The card moves whole: a range
  keeps its length and a moment its time of day on the reader's clock
  (C10; `movedValue`), in one change that one undo takes back. A card
  without a date gets the day picked. Picking a day is the change; picking
  the day it is on, Escape or a press outside leave the card as it was.
  Other ends and times are edited in the card itself, with the date
  property's own picker.

  The calendar names the card (`cardId`) from its menu, once the list the
  menu was in has closed, and then opens it: a popover next to the picked
  day (or "No date", for a card without one), or a sheet on a compact page.
  In the popover the focus goes to the card's day (the picked day, for a
  card without one), where the arrow keys walk the month; a sheet takes it
  itself, as every sheet does. Once closed, the calendar puts the focus on
  the card's day, or on "No date" (`onclosed`).
  Once the board has kept the change, the calendar goes to the new day
  (`onmoved`) and a screen reader hears where the card went; a change the
  board refuses is put back by the board, and nothing moves or is said.
-->
<script lang="ts">
  import { parseDate, type DateValue as PickedDay } from '@internationalized/date';

  import BottomSheet from '$lib/components/sheet/BottomSheet.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { insets } from '$lib/layout/room.svelte.ts';
  import type { CalendarDate } from '$lib/logic/dates.ts';
  import { displayDay, displayDayLong } from '$lib/logic/format.ts';
  import type { Card, PropertyTemplate } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import { Calendar, Day as CalendarDay } from '$lib/ui/calendar/index.js';
  import * as Popover from '$lib/ui/popover/index.js';

  import type { Placed } from './placed.ts';
  import { moveCard } from './write.ts';

  interface Props {
    /** The card whose date is being changed. Bindable: none once closed. */
    cardId: string | undefined;
    open: boolean;
    board: BoardStore;
    placed: Placed;
    /** The date property the calendar is placed by; the picker is for nothing else. */
    template: PropertyTemplate | undefined;
    /** The calendar's picked day: where the popover hangs, and the month a card without a date starts at. */
    selected: CalendarDate;
    today: CalendarDate;
    sheet: boolean;
    /** The board kept the card's move to a day: the calendar goes there. */
    onmoved: (date: CalendarDate) => void;
    /** Closed. `dated`: the card has a date now, else the focus goes back to "No date". */
    onclosed: (dated: boolean) => void;
  }

  let {
    cardId = $bindable(),
    open = $bindable(),
    board,
    placed,
    template,
    selected,
    today,
    sheet,
    onmoved,
    onclosed,
  }: Props = $props();

  const uid = $props.id();
  /** Said to a screen reader once a card has moved. */
  let announced = $state('');
  let box = $state<HTMLElement | null>(null);

  const card = $derived(cardId === undefined ? undefined : placed.card(cardId));
  const span = $derived(cardId === undefined ? undefined : placed.span(cardId));
  const locale = $derived(placed.locale);
  const title = $derived.by(() => {
    const name = card ? placed.title(card) : '';
    return span
      ? m.calendar_change_date_of({ title: name })
      : m.calendar_set_date_of({ title: name });
  });
  const hint = $derived.by(() => {
    if (!card || !span) return m.calendar_set_hint({ property: template?.name ?? '' });
    const days = placed.range(card) ?? displayDay(span.first, locale);
    const time = placed.time(card);
    return m.calendar_move_hint({
      days: time ? m.calendar_time_title({ time, title: days }) : days,
    });
  });
  /** The anchor is read when the popover opens: the day the card's list was of, or "No date". */
  const anchor = $derived(
    span ? `[data-testid="calendar"] [data-day="${selected}"]` : '[data-testid="calendar-undated"]',
  );
  const weekStartsOn = $derived((placed.regional.weekStart % 7) as 0 | 1 | 2 | 3 | 4 | 5 | 6);

  /**
   * A day was picked. The card's own day comes back as none (the calendar
   * takes a second press on it for "pick nothing"): that closes as Escape does.
   */
  function pick(picked: PickedDay | undefined): void {
    const date = picked?.toString() as CalendarDate | undefined;
    // For a card without a date the calendar also says "none" as it starts: nothing was pressed.
    if (date === undefined && !span) return;
    open = false;
    if (card && date !== undefined && date !== span?.first && template?.type === 'date') {
      void move(card, template, date);
    }
    // A sheet closed by its owner does not say so; a popover does, on giving up the focus.
    if (sheet) closed();
  }

  async function move(moving: Card, property: PropertyTemplate, date: CalendarDate): Promise<void> {
    announced = '';
    const kept = await moveCard(board, moving, property, date, placed.regional.timeZone);
    if (!kept) return;
    onmoved(date);
    // Emptied first, so that the same words again are said again.
    announced = m.calendar_moved({
      title: placed.title(moving),
      day: displayDayLong(date, locale),
    });
  }

  function closed(): void {
    const id = cardId;
    cardId = undefined;
    onclosed(id !== undefined && placed.span(id) !== undefined);
  }

  /** The focus goes to the card's day, or for a card without one to the calendar's picked day. */
  function focusDay(event: Event): void {
    event.preventDefault();
    const day = span?.first ?? selected;
    (
      box?.querySelector<HTMLElement>(`[data-bits-day][data-value="${day}"]`) ??
      box?.querySelector<HTMLElement>('[data-bits-day][tabindex="0"]')
    )?.focus();
  }

  // The calendar marks the device's today; the reader's is marked instead.
  const NOT_TODAY =
    '[&[data-today]:not([data-selected])]:bg-transparent [&[data-today]:not([data-selected])]:text-inherit';
  const TODAY =
    'font-semibold not-data-selected:bg-accent not-data-selected:text-accent-foreground';
</script>

<p class="sr-only" role="status" data-testid="calendar-status">{announced}</p>

{#snippet picker()}
  <div data-testid="day-picker" class="flex flex-col gap-1 p-1">
    <p id="{uid}-hint" class="max-w-72 px-2 pt-1 text-xs text-muted-foreground">{hint}</p>
    <Calendar
      type="single"
      bind:value={() => (span ? parseDate(span.first) : undefined), pick}
      placeholder={parseDate(span?.first ?? selected)}
      {locale}
      {weekStartsOn}
      fixedWeeks
      calendarLabel={title}
      aria-describedby="{uid}-hint"
      class="mx-auto bg-transparent p-1 roomy:[--cell-size:var(--spacing-touch)]"
    >
      {#snippet day({ day: cell }: { day: PickedDay })}
        <CalendarDay class={cell.toString() === today ? TODAY : NOT_TODAY} />
      {/snippet}
    </Calendar>
  </div>
{/snippet}

<!-- Kept while a card is named, even one that has just gone: the surface closes, and says so. -->
{#if cardId !== undefined}
  {#if sheet}
    <BottomSheet
      bind:open
      {title}
      onOpenChange={(next: boolean) => {
        if (!next) closed();
      }}
    >
      {@render picker()}
    </BottomSheet>
  {:else}
    <Popover.Root bind:open>
      <Popover.Content
        bind:ref={box}
        customAnchor={anchor}
        side="bottom"
        align="start"
        collisionPadding={insets.collisionPadding}
        aria-label={title}
        class="w-auto max-w-[calc(100vw-1rem-var(--safe-left)-var(--safe-right))] p-0"
        onOpenAutoFocus={focusDay}
        onCloseAutoFocus={(event: Event) => {
          event.preventDefault();
          closed();
        }}
      >
        {@render picker()}
      </Popover.Content>
    </Popover.Root>
  {/if}
{/if}
