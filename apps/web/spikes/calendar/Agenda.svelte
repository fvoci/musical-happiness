<!--
  Days as a list, each with its cards under it: what a box too narrow for
  bars shows instead of a grid, in one of the two proposals for the month and
  always for the view of one week.

  A day with nothing on it is left out, except today and the picked day
  (`every` lists them all: a week is short enough). A card of several days is
  listed under each of them and says which of its days that is.

  Every day has its own "+": the day a new card is made on is never in doubt.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';

  import type { CalendarDate } from '$lib/logic/dates.ts';

  import IconButton from '../mobile/IconButton.svelte';

  import CardRow from './CardRow.svelte';
  import { SHOWN } from './data.ts';
  import { dayLabel, dayLongLabel } from './names.ts';
  import { SHOW } from './show.ts';
  import { T } from './text.ts';
  import { view } from './view.svelte.ts';

  interface Props {
    days: readonly CalendarDate[];
    /** Days without a card are listed too. */
    every?: boolean;
    /** Menus are bottom sheets. */
    sheet: boolean;
  }

  let { days, every = false, sheet }: Props = $props();

  const listed = $derived(
    days
      .map((date) => ({ date, entries: view.on(date) }))
      .filter(
        ({ date, entries }) =>
          every || entries.length > 0 || date === view.today || date === view.selected,
      ),
  );
</script>

<div
  data-testid="agenda"
  role="region"
  aria-label={T.agenda}
  class="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-(--safe-bottom)"
>
  {#each listed as { date, entries } (date)}
    <section data-agenda-day={date} class="border-b px-1 py-1">
      <header class="flex items-center gap-2 pl-3">
        <h3 class={['font-semibold', date === view.selected && 'underline underline-offset-4']}>
          {dayLongLabel(date)}
        </h3>
        {#if date === view.today}
          <span
            class="rounded-full bg-primary px-2 py-0.5 text-xs font-medium text-primary-foreground"
          >
            {T.today}
          </span>
        {/if}
        <div class="flex-1"></div>
        <IconButton label={T.newCardOn(dayLabel(date))}><PlusIcon /></IconButton>
      </header>
      {#each entries as entry (entry.id)}
        <CardRow
          {entry}
          day={date}
          {sheet}
          opened={entry.id === SHOWN.moved ? SHOW.cardMenu : 'none'}
        />
      {:else}
        <p class="px-3 pb-2 text-muted-foreground">{T.noCards}</p>
      {/each}
    </section>
  {/each}
</div>
