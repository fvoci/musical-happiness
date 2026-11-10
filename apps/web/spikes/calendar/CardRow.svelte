<!--
  One card as a line of a list: under a day, in the list of one day, among
  the cards without a date. The line opens the card; "…" at its end has the
  commands, and "change the date" among them is how a card moves to another
  day without being dragged.

  A line is 2.5rem high for a mouse and 3rem for a finger.
-->
<script lang="ts">
  import CalendarIcon from '@lucide/svelte/icons/calendar';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import LinkIcon from '@lucide/svelte/icons/link';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { untrack } from 'svelte';

  import { daysBetween, type CalendarDate } from '$lib/logic/dates.ts';

  import ActionMenu from '../mobile/ActionMenu.svelte';
  import type { Action } from '../mobile/actions.ts';
  import Avatar from '../mobile/Avatar.svelte';
  import Chip from '../mobile/Chip.svelte';
  import { STATUSES, optionOf, personOf } from '../mobile/data.ts';
  import DatePicker from '../mobile/DatePicker.svelte';
  import IconButton from '../mobile/IconButton.svelte';
  import Surface from '../mobile/Surface.svelte';
  import { T as MOBILE } from '../mobile/text.ts';

  import type { Entry } from './data.ts';
  import { rangeLabel, timeLabel } from './names.ts';
  import { T } from './text.ts';
  import { view } from './view.svelte.ts';

  interface Props {
    entry: Entry;
    /** The day the line is listed under: a card of several days says which of them this is. */
    day?: CalendarDate;
    /** Menus are bottom sheets. */
    sheet: boolean;
    /** What of the line is open when the page loads. */
    opened?: 'none' | 'menu' | 'pick';
    /** The card is being dragged somewhere else: the line stays behind, faded. */
    lifted?: boolean;
  }

  let { entry, day, sheet, opened = 'none', lifted = false }: Props = $props();

  let menuOpen = $state(untrack(() => opened === 'menu'));
  let pickerOpen = $state(untrack(() => opened === 'pick'));
  /** The date picker stands where the menu was once "change the date" is chosen. */
  const picking = untrack(() => opened === 'pick');

  const status = $derived(optionOf(STATUSES, entry.status));
  const person = $derived(personOf(entry.assignee));
  const days = $derived(view.daysOf(entry));
  const time = $derived(timeLabel(entry.value?.from, view.timeZone));
  const length = $derived(days ? daysBetween(days.first, days.last) + 1 : 0);

  const actions = $derived<Action[]>([
    { id: 'date', label: days ? T.changeDate : T.setDate, icon: CalendarIcon },
    { id: 'duplicate', label: MOBILE.duplicate, icon: CopyIcon },
    { id: 'link', label: MOBILE.copyLink, icon: LinkIcon },
    { id: 'delete', label: MOBILE.deleteCard, icon: TrashIcon, destructive: true, separated: true },
  ]);
</script>

<div
  data-card={entry.id}
  class={['flex items-center gap-0.5 rounded-lg hover:bg-muted', lifted && 'opacity-40']}
>
  <button
    type="button"
    class="flex min-h-10 min-w-0 flex-1 items-center gap-2 rounded-lg py-1 pr-1 pl-3 text-left pointer-coarse:min-h-12"
  >
    <span class="flex min-w-0 flex-1 flex-col">
      <span class="truncate">{entry.title}</span>
      <span class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
        {#if status}<Chip option={status} />{/if}
        {#if time}<span class="shrink-0">{time}</span>{/if}
        {#if days && length > 1}
          <span class="truncate">
            {rangeLabel(days.first, days.last)}
            {#if day}· {T.continued(daysBetween(days.first, day) + 1, length)}{/if}
          </span>
        {/if}
      </span>
    </span>
    {#if person}<Avatar {person} />{/if}
  </button>

  {#if picking}
    <Surface
      bind:open={pickerOpen}
      {sheet}
      title={T.changeDate}
      subtitle={entry.title}
      popoverClass="w-auto"
    >
      {#snippet trigger(props: Record<string, unknown>)}
        <IconButton {...props} label={T.cardMenu(entry.title)} class="shrink-0">
          <EllipsisIcon />
        </IconButton>
      {/snippet}
      <DatePicker />
    </Surface>
  {:else}
    <ActionMenu
      bind:open={menuOpen}
      {sheet}
      title={MOBILE.cardMenu}
      subtitle={entry.title}
      {actions}
    >
      {#snippet trigger(props: Record<string, unknown>)}
        <IconButton {...props} label={T.cardMenu(entry.title)} class="shrink-0">
          <EllipsisIcon />
        </IconButton>
      {/snippet}
    </ActionMenu>
  {/if}
</div>
