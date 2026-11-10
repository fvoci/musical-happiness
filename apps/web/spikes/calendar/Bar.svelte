<!--
  One card in the grid: a bar across the days it is on, with its title. The
  part of a card that falls in one week is one bar; an end that goes on into
  another week is cut square and marked with an arrow.

  - With a mouse (`pressable`) the bar is a button that opens the card, and
    "…" appears at its end when it is pointed at.
  - With a finger the bar is not a target: it is too low to hit, and the cell
    under it takes the tap and opens the cards of that day. Only in the view
    of one week (`tall`), where a bar is a block of two lines, is it a button
    for a finger too.
-->
<script lang="ts">
  import CalendarIcon from '@lucide/svelte/icons/calendar';
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import LinkIcon from '@lucide/svelte/icons/link';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { untrack } from 'svelte';

  import ActionMenu from '../mobile/ActionMenu.svelte';
  import type { Action } from '../mobile/actions.ts';
  import Avatar from '../mobile/Avatar.svelte';
  import { STATUSES, optionOf, personOf } from '../mobile/data.ts';
  import DatePicker from '../mobile/DatePicker.svelte';
  import Surface from '../mobile/Surface.svelte';
  import { T as MOBILE } from '../mobile/text.ts';

  import { STATUS_FILLS, type Entry } from './data.ts';
  import { rangeLabel, timeLabel } from './names.ts';
  import { SHOW } from './show.ts';
  import { T } from './text.ts';
  import { view } from './view.svelte.ts';

  interface Props {
    entry: Entry;
    /** The card began before this week, or goes on after it. */
    fromBefore?: boolean;
    toAfter?: boolean;
    /** A block of two lines, as in the view of one week. */
    tall?: boolean;
    /** The bar itself is a button. */
    pressable: boolean;
    /** Menus are bottom sheets. */
    sheet: boolean;
    /** What of the bar is open when the page loads. */
    opened?: 'none' | 'menu' | 'pick';
    /** The card is being dragged: this is what stays behind. */
    lifted?: boolean;
    /** Drawn as picked, with a handle at each end the card really ends at. */
    handles?: boolean;
    /** A picture of the bar and nothing more: the copy that follows the pointer in a drag. */
    inert?: boolean;
  }

  let {
    entry,
    fromBefore = false,
    toAfter = false,
    tall = false,
    pressable,
    sheet,
    opened = 'none',
    lifted = false,
    handles = false,
    inert = false,
  }: Props = $props();

  let menuOpen = $state(untrack(() => opened === 'menu'));
  let pickerOpen = $state(untrack(() => opened === 'pick'));
  const picking = untrack(() => opened === 'pick');

  const status = $derived(optionOf(STATUSES, entry.status));
  const person = $derived(personOf(entry.assignee));
  const days = $derived(view.daysOf(entry));
  const time = $derived(timeLabel(entry.value?.from, view.timeZone));
  const ranged = $derived(days !== undefined && days.first !== days.last);

  /** For a screen reader: the title, and the days when there are several. */
  const name = $derived(
    days && ranged ? `${entry.title}, ${rangeLabel(days.first, days.last)}` : entry.title,
  );

  const fill = $derived(
    SHOW.tinted && status
      ? STATUS_FILLS[status.hue]
      : 'bg-foreground/10 can-hover:hover:bg-foreground/15',
  );

  const actions: Action[] = [
    { id: 'date', label: T.changeDate, icon: CalendarIcon },
    { id: 'duplicate', label: MOBILE.duplicate, icon: CopyIcon },
    { id: 'link', label: MOBILE.copyLink, icon: LinkIcon },
    { id: 'delete', label: MOBILE.deleteCard, icon: TrashIcon, destructive: true, separated: true },
  ];

  const MORE =
    'absolute inset-y-0 right-0 flex w-6 items-center justify-center rounded-sm bg-muted text-foreground focus-visible:opacity-100 aria-expanded:opacity-100 can-hover:opacity-0 can-hover:group-hover/bar:opacity-100';
</script>

{#snippet content()}
  {#if fromBefore}<ChevronLeftIcon aria-hidden="true" class="size-3 shrink-0 opacity-60" />{/if}
  {#if tall}
    <span class="flex min-w-0 flex-1 flex-col gap-0.5 py-1">
      <span class="line-clamp-2 leading-snug">{entry.title}</span>
      <span class="flex min-w-0 items-center gap-1 text-muted-foreground">
        {#if person}<Avatar {person} decorative class="size-4 text-[0.625rem]" />{/if}
        <span class="truncate">{time ?? status?.label}</span>
      </span>
    </span>
  {:else}
    {#if time}<span class="shrink-0 text-muted-foreground">{time}</span>{/if}
    <span class="truncate">{entry.title}</span>
  {/if}
  {#if toAfter}
    <ChevronRightIcon aria-hidden="true" class="ml-auto size-3 shrink-0 opacity-60" />
  {/if}
{/snippet}

<div
  data-bar={entry.id}
  class={[
    'group/bar relative flex min-w-0 rounded-sm text-xs text-foreground',
    tall ? 'min-h-11' : 'h-5.5',
    fill,
    fromBefore && 'rounded-l-none',
    toAfter && 'rounded-r-none',
    lifted && 'opacity-40',
    handles && 'ring-2 ring-ring',
  ]}
>
  {#if pressable && !inert}
    <button
      type="button"
      aria-label={name}
      class="flex min-w-0 flex-1 items-center gap-1 rounded-sm px-1.5 text-left"
    >
      {@render content()}
    </button>
    {#if !toAfter && !tall}
      {#if picking}
        <Surface
          bind:open={pickerOpen}
          {sheet}
          title={T.changeDate}
          subtitle={entry.title}
          popoverClass="w-auto"
        >
          {#snippet trigger(props: Record<string, unknown>)}
            <button {...props} type="button" aria-label={T.cardMenu(entry.title)} class={MORE}>
              <EllipsisIcon class="size-3.5" />
            </button>
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
            <button {...props} type="button" aria-label={T.cardMenu(entry.title)} class={MORE}>
              <EllipsisIcon class="size-3.5" />
            </button>
          {/snippet}
        </ActionMenu>
      {/if}
    {/if}
  {:else}
    <span class="flex min-w-0 flex-1 items-center gap-1 px-1.5">
      {@render content()}
    </span>
  {/if}

  {#if handles}
    <!-- Dragging an end changes the first or the last day. Only an end the card really has. -->
    {#if !fromBefore}
      <span
        aria-hidden="true"
        title={T.resizeStart}
        class="absolute top-1/2 -left-1 h-4 w-2 -translate-y-1/2 cursor-ew-resize rounded-full border-2 border-background bg-ring"
      ></span>
    {/if}
    {#if !toAfter}
      <span
        aria-hidden="true"
        title={T.resizeEnd}
        class="absolute top-1/2 -right-1 h-4 w-2 -translate-y-1/2 cursor-ew-resize rounded-full border-2 border-background bg-ring"
      ></span>
    {/if}
  {/if}
</div>
