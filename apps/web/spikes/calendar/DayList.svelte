<!--
  The cards of one day, with their titles in full: what a day of the grid
  opens. The same content wherever it is put: under the small grid of a
  narrow box (`inline`), in a popover next to the cell, or in a column beside
  the grid (both with a button that closes it).

  The last line makes a card on this day (under the small grid the round
  button does that, and the line is left out). In the proposal where pressing
  an empty cell already made one, that card is the last line and waits for
  its title; the field has 16 pixels of text for a finger, so that iOS does
  not zoom in on it.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';
  import XIcon from '@lucide/svelte/icons/x';

  import type { CalendarDate } from '$lib/logic/dates.ts';

  import IconButton from '../mobile/IconButton.svelte';

  import CardRow from './CardRow.svelte';
  import { SHOWN } from './data.ts';
  import { dayLabel, dayLongLabel } from './names.ts';
  import { SHOW } from './show.ts';
  import { T } from './text.ts';
  import { view } from './view.svelte.ts';

  interface Props {
    date: CalendarDate;
    /** Menus are bottom sheets. */
    sheet: boolean;
    /** Given where the list can be closed: a popover, a column. */
    onclose?: () => void;
    /** The last line makes a card on this day. Not where a round button under the list does. */
    adder?: boolean;
  }

  let { date, sheet, onclose, adder = true }: Props = $props();

  const entries = $derived(view.on(date));
  /** Pressing the empty cell has made a card here. */
  const made = $derived(SHOW.creating && SHOW.create === 'cell' && date === SHOWN.empty);
</script>

<section data-testid="day-list" data-date={date} class="flex min-h-0 flex-1 flex-col">
  <header class="flex shrink-0 items-center gap-2 py-1 pr-1 pl-4">
    <h3 class="font-semibold">{dayLongLabel(date)}</h3>
    {#if date === view.today}
      <span class="rounded-full bg-primary px-2 py-0.5 text-xs font-medium text-primary-foreground">
        {T.today}
      </span>
    {/if}
    <div class="flex-1"></div>
    {#if onclose}
      <IconButton label={T.closeDay} onclick={onclose}><XIcon /></IconButton>
    {:else}
      <!-- Keeps the row as high as where there is a button. -->
      <div class="h-8 pointer-coarse:h-touch"></div>
    {/if}
  </header>

  <div class={['min-h-0 flex-1 overflow-y-auto overscroll-contain px-1', adder ? 'pb-2' : 'pb-24']}>
    {#each entries as entry (entry.id)}
      <CardRow
        {entry}
        day={date}
        {sheet}
        opened={entry.id === SHOWN.moved ? SHOW.cardMenu : 'none'}
        lifted={SHOW.dragging && entry.id === SHOWN.moved}
      />
    {:else}
      {#if !made}
        <p class="px-3 py-2 text-muted-foreground">{T.noCards}</p>
      {/if}
    {/each}

    {#if made}
      <div class="px-2 py-1">
        <input
          data-testid="new-card-title"
          aria-label={T.newCardOn(dayLabel(date))}
          placeholder={T.untitled}
          class="h-9 w-full rounded-lg bg-background px-3 ring-2 ring-ring outline-none placeholder:text-muted-foreground pointer-coarse:h-touch pointer-coarse:text-base"
        />
      </div>
    {:else if adder}
      <button
        type="button"
        data-testid="new-card-on-day"
        class="flex min-h-10 w-full items-center gap-2 rounded-lg px-3 text-left text-muted-foreground hover:bg-muted hover:text-foreground pointer-coarse:min-h-12"
      >
        <PlusIcon class="size-4 shrink-0" />
        {T.newCardOn(dayLabel(date))}
      </button>
    {/if}
  </div>
</section>
