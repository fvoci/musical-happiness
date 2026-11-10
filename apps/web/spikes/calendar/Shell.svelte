<!--
  One mock-up: the board page of the mobile mock-ups with the calendar view
  in its pane, put together the way the address asks for.

  The page is the same row as there (the list of boards, the pane), and the
  same stages decide what the row is made of. Inside the pane, under the bar
  of the board and the header of the view, is the calendar: a row of its own
  controls, and a box that the grid follows the width of (`room`). What is in
  that box:

  - 34rem and wider: the weeks of the month as a grid of bars, or one week as
    high as the box.
  - narrower: a small grid of numbers and dots over the cards of the picked
    day; or, as the second proposal for the month and always for a week, a
    strip of one week over a list of days.

  The cards of a day picked in the grid of bars open in a popover next to
  the cell, or (second proposal) in a column at the right. That column, and
  the drawer of cards without a date, lie over the grid in a pane narrower
  than 54rem and stand beside it from there; beside it they narrow the box,
  and the grid in it changes as if the window had been made that much
  smaller.

  The month in view and the day picked are in `view`, outside of all of
  these parts: a window that changes size swaps the parts and keeps both.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';

  import { Button } from '$lib/ui/button/index.js';
  import * as Popover from '$lib/ui/popover/index.js';

  import AppBar from '../mobile/AppBar.svelte';
  import AppSidebar from '../mobile/AppSidebar.svelte';
  import BottomSheet from '../mobile/BottomSheet.svelte';
  import { app, coarse, insets, pane } from '../mobile/layout.svelte.ts';

  import Agenda from './Agenda.svelte';
  import { CURRENT } from './current.ts';
  import { SHOWN, UNDATED, entryOf } from './data.ts';
  import DayList from './DayList.svelte';
  import MiniMonth from './MiniMonth.svelte';
  import MonthGrid from './MonthGrid.svelte';
  import { dayLabel, dayLongLabel } from './names.ts';
  import { room } from './room.svelte.ts';
  import { SHOW } from './show.ts';
  import { T } from './text.ts';
  import Toolbar from './Toolbar.svelte';
  import Undated from './Undated.svelte';
  import ViewHeader from './ViewHeader.svelte';
  import { view } from './view.svelte.ts';
  import WeekStrip from './WeekStrip.svelte';

  const stage = $derived(app.stage);
  /** Menus and pickers are bottom sheets. */
  const sheet = $derived(stage === 'compact');

  let drawerOpen = $state(false);
  let pinned = $state(!CURRENT.sidebarAsStrip);
  const sidebarForm = $derived(
    stage === 'expanded' ? (pinned ? 'fixed' : 'strip') : stage === 'medium' ? 'strip' : 'none',
  );
  const overlay = $derived(stage !== 'expanded');
  const leading = $derived(
    sidebarForm === 'fixed' ? 'toggle' : sidebarForm === 'strip' ? 'none' : 'menu',
  );
  $effect(() => {
    if (!overlay) drawerOpen = false;
  });

  function openList(): void {
    if (stage === 'expanded') pinned = !pinned;
    else drawerOpen = true;
  }

  /**
   * The cards of the picked day are open (a popover or a column; under a
   * small grid they are always there). A finger reaches the menu of a card
   * and the way to make one through this list, so the screens about those
   * start with it open.
   */
  let dayOpen = $state(
    SHOW.dayOpen || ((SHOW.creating || SHOW.cardMenu !== 'none') && coarse.current),
  );
  let undatedOpen = $state(SHOW.undatedOpen && SHOW.undated === 'drawer');

  /** The days the list of a narrow box goes through: a week, or from the picked week to the end of the grid. */
  const listed = $derived.by(() => {
    if (view.mode === 'week') return view.week;
    const from = view.week[0];
    return view.weeks.flat().filter((date) => !from || date >= from);
  });

  const moved = entryOf(SHOWN.moved);

  function pick(): void {
    undatedOpen = false;
    dayOpen = true;
  }

  function toggleUndated(): void {
    undatedOpen = !undatedOpen;
    if (undatedOpen) dayOpen = false;
  }
</script>

<div
  data-testid="app"
  data-stage={stage}
  class="@container/app flex h-[calc(100dvh-var(--safe-top))] min-h-0 overflow-hidden"
>
  <AppSidebar
    form={sidebarForm}
    {overlay}
    bind:drawerOpen
    onexpand={openList}
    onfind={() => undefined}
  />

  <main
    data-testid="pane"
    class="@container/pane relative flex min-w-0 flex-1 flex-col overflow-hidden bg-background"
    {@attach pane.attach}
  >
    <AppBar {leading} onmenu={openList} onswitch={() => undefined} />

    <!-- Nothing under the bar is drawn before the pane is measured: what opens with the page depends on its width. -->
    {#if pane.measured}
      <ViewHeader {sheet} byOpen={SHOW.byOpen} />
      <Toolbar {undatedOpen} onundated={toggleUndated} />

      <div data-testid="calendar-body" class="relative flex min-h-0 flex-1">
        <div
          data-testid="calendar-box"
          data-month="{view.month.year}-{view.month.month}"
          data-selected={view.selected}
          class="relative flex min-w-0 flex-1 flex-col"
          {@attach room.attach}
        >
          <!-- The grid or the list, and what floats over it. The strip of cards without a date is under this. -->
          <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden">
            {#if !room.measured}
              <!-- Measured first. -->
            {:else if room.bars}
              <MonthGrid
                weeks={view.mode === 'week' ? [view.week] : view.weeks}
                tall={view.mode === 'week'}
                {sheet}
                onpick={pick}
              />
            {:else if view.agenda}
              <WeekStrip />
              <Agenda days={listed} every={view.mode === 'week'} {sheet} />
            {:else}
              <MiniMonth />
              <div class="flex min-h-0 flex-1 flex-col border-t pb-(--safe-bottom)">
                <DayList date={view.selected} {sheet} adder={false} />
              </div>
            {/if}

            {#if SHOW.dragging && moved && coarse.current}
              <!-- A finger covers the day it is over: the day is named where it can be read. -->
              <p
                data-testid="drop-hint"
                role="status"
                class="pointer-events-none absolute inset-x-0 bottom-[calc(1rem+var(--safe-bottom))] z-30 mx-auto w-fit rounded-full bg-foreground px-4 py-2 text-background shadow-lg"
              >
                {T.dragging(dayLabel(SHOWN.dropOn))}
              </p>
            {/if}

            <!--
              "New" within reach of the thumb where the header of the view is
              folded, as in every view. Here it makes a card on the picked day.
              Not over the list of days, where every day has a "+" of its own.
            -->
            {#if !pane.header && !view.agenda && !(SHOW.dragging && coarse.current)}
              <Button
                aria-label={T.newCardOn(dayLabel(view.selected))}
                data-testid="new-card-fab"
                class={[
                  'absolute right-4 z-20 size-14 rounded-full shadow-lg',
                  // The strip under it keeps clear of the home indicator itself.
                  SHOW.undated === 'strip' ? 'bottom-4' : 'bottom-[calc(1rem+var(--safe-bottom))]',
                ]}
              >
                <PlusIcon class="size-6" />
              </Button>
            {/if}
          </div>

          {#if SHOW.undated === 'strip'}
            <Undated form="strip" open={SHOW.undatedOpen} {sheet} />
          {/if}
        </div>

        {#if room.bars && SHOW.dayList === 'panel' && dayOpen}
          <aside
            data-testid="day-panel"
            aria-label={dayLongLabel(view.selected)}
            class="@day-beside/pane:static @day-beside/pane:shadow-none absolute inset-y-0 right-0 z-30 flex w-80 max-w-full flex-col border-l bg-background shadow-lg"
          >
            <DayList
              date={view.selected}
              {sheet}
              onclose={() => {
                dayOpen = false;
              }}
            />
          </aside>
        {:else if undatedOpen && !sheet}
          <aside
            data-testid="undated-panel"
            aria-label={T.undated}
            class="@day-beside/pane:static @day-beside/pane:shadow-none absolute inset-y-0 right-0 z-30 flex w-80 max-w-full flex-col border-l bg-background shadow-lg"
          >
            <Undated
              form="list"
              {sheet}
              onclose={() => {
                undatedOpen = false;
              }}
            />
          </aside>
        {/if}
      </div>
    {/if}
  </main>
</div>

{#if room.bars && SHOW.dayList === 'popover'}
  <Popover.Root bind:open={dayOpen}>
    <Popover.Content
      customAnchor={`[data-testid="month-grid"] [data-day="${view.selected}"]`}
      side="right"
      align="start"
      collisionPadding={{
        top: 8 + insets.top,
        right: 8 + insets.right,
        bottom: 8 + insets.bottom,
        left: 8 + insets.left,
      }}
      aria-label={dayLongLabel(view.selected)}
      data-testid="day-popover"
      class="max-h-[min(30rem,calc(100dvh-1rem))] w-80 max-w-[calc(100vw-1rem-var(--safe-left)-var(--safe-right))] gap-0 p-0 pb-1"
      onOpenAutoFocus={(event: Event) => {
        // A finger has not asked for the keyboard, and the ring of the keyboard is not wanted either.
        if (coarse.current) event.preventDefault();
      }}
    >
      <DayList
        date={view.selected}
        {sheet}
        onclose={() => {
          dayOpen = false;
        }}
      />
    </Popover.Content>
  </Popover.Root>
{/if}

{#if sheet}
  <BottomSheet bind:open={undatedOpen} title={T.undatedCount(UNDATED.length)}>
    <Undated form="list" {sheet} />
  </BottomSheet>
{/if}
