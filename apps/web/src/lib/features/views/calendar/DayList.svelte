<!--
  The cards of one day, with their titles in full: what a day of the grid
  opens. The same content wherever it is put: under the small grid of a
  narrow box, or in a popover next to the cell (then with a button that
  closes it; question C3, answer A).

  A line is a link that opens the card: its title, its time when its value
  has one, and for a card of several days which of them this is ("November
  9 – 13 · Day 2 of 5"). Under it, the properties the view turned on (C5).
-->
<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';

  import { faceProperties } from '$lib/components/card-face/face.ts';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { PropertyValue, type PropertyUser } from '$lib/components/properties/index.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { daysBetween, type CalendarDate } from '$lib/logic/dates.ts';
  import { displayDayLong } from '$lib/logic/format.ts';
  import type { PropertyTemplate } from '$lib/logic/wire.ts';

  import type { Placed } from './placed.ts';

  interface Props {
    date: CalendarDate;
    placed: Placed;
    today: CalendarDate;
    templates: readonly PropertyTemplate[];
    visiblePropertyIds: readonly string[];
    users: readonly PropertyUser[];
    nameFormat: string;
    cardHref: (cardId: string) => string;
    onOpenCard: (cardId: string) => void;
    /** Given where the list can be closed: a popover. */
    onclose?: () => void;
  }

  let {
    date,
    placed,
    today,
    templates,
    visiblePropertyIds,
    users,
    nameFormat,
    cardHref,
    onOpenCard,
    onclose,
  }: Props = $props();

  const uid = $props.id();
  const cards = $derived(placed.on(date));

  function open(event: MouseEvent, cardId: string): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
      return;
    event.preventDefault();
    onOpenCard(cardId);
  }

  /** "Day 2 of 5", for a card of several days. */
  function dayOf(cardId: string): string | undefined {
    const span = placed.span(cardId);
    if (!span || span.first === span.last) return undefined;
    const count = daysBetween(span.first, span.last) + 1;
    return m.calendar_day_of({ nth: daysBetween(span.first, date) + 1, count });
  }
</script>

<section
  data-testid="day-list"
  data-date={date}
  aria-labelledby="{uid}-title"
  class="flex min-h-0 flex-1 flex-col"
>
  <header class="flex shrink-0 items-center gap-2 py-1 pr-1 pl-4">
    <h3 id="{uid}-title" class="font-semibold">{displayDayLong(date, placed.locale)}</h3>
    {#if date === today}
      <span class="rounded-full bg-primary px-2 py-0.5 text-xs font-medium text-primary-foreground">
        {m.calendar_today()}
      </span>
    {/if}
    <div class="flex-1"></div>
    {#if onclose}
      <IconButton label={m.calendar_close_day()} data-testid="day-list-close" onclick={onclose}>
        <XIcon />
      </IconButton>
    {:else}
      <!-- Keeps the row as high as where there is a button. -->
      <div class="h-8 roomy:h-touch"></div>
    {/if}
  </header>

  <ul class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1 pb-2">
    {#each cards as card (card.id)}
      {@const time = placed.time(card)}
      {@const range = placed.range(card)}
      {@const shown = faceProperties(card, templates, visiblePropertyIds)}
      <li data-card={card.id} class="rounded-lg hover:bg-muted">
        <a
          href={cardHref(card.id)}
          class="flex min-h-10 min-w-0 flex-col justify-center rounded-lg px-3 py-1 outline-none focus-visible:ring-2 focus-visible:ring-ring roomy:min-h-12"
          onclick={(event) => {
            open(event, card.id);
          }}
        >
          <span class={['truncate', !card.title && 'text-muted-foreground']}>
            {placed.title(card)}
          </span>
          {#if time || range}
            <span class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
              {#if time}<span class="shrink-0">{time}</span>{/if}
              {#if range}<span class="truncate">{range} · {dayOf(card.id)}</span>{/if}
            </span>
          {/if}
        </a>
        {#if shown.length > 0}
          <div class="flex min-w-0 flex-wrap items-center gap-x-1 px-1 pb-1 text-xs">
            {#each shown as template (template.id)}
              <div data-property={template.id} title={template.name} class="min-w-0">
                <span class="sr-only">{template.name}:</span>
                <PropertyValue
                  {template}
                  {card}
                  value={card.fields.properties[template.id]}
                  mode="display"
                  {users}
                  {nameFormat}
                  locale={placed.locale}
                  regional={placed.regional}
                />
              </div>
            {/each}
          </div>
        {/if}
      </li>
    {:else}
      <li class="px-3 py-2 text-muted-foreground">{m.calendar_no_cards()}</li>
    {/each}
  </ul>
</section>
