<!--
  One card in the grid: a bar across the days of one week it is on, with its
  title (and its time, when its value has one). An end that goes on into
  another week is cut square and marked with an arrow.

  With a mouse the bar is a link that opens the card. With a finger it is a
  picture (`pressable` false): 22 pixels are too low to hit, and the cell
  under it takes the tap and opens the cards of the day (question C2).
-->
<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';

  interface Props {
    cardId: string;
    title: string;
    /** For a screen reader: the title, and the days when there are several. */
    name: string;
    /** The time of a value that has one, on the reader's clock. */
    time?: string;
    /** The card began before this week, or goes on after it. */
    fromBefore: boolean;
    toAfter: boolean;
    pressable: boolean;
    href: string;
    onopen: (cardId: string) => void;
  }

  let { cardId, title, name, time, fromBefore, toAfter, pressable, href, onopen }: Props = $props();

  function open(event: MouseEvent): void {
    // A new tab or window is the browser's to open.
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
      return;
    event.preventDefault();
    onopen(cardId);
  }
</script>

{#snippet content()}
  {#if fromBefore}<ChevronLeftIcon aria-hidden="true" class="size-3 shrink-0 opacity-60" />{/if}
  {#if time}<span class="shrink-0 text-muted-foreground">{time}</span>{/if}
  <span class="truncate">{title}</span>
  {#if toAfter}
    <ChevronRightIcon aria-hidden="true" class="ml-auto size-3 shrink-0 opacity-60" />
  {/if}
{/snippet}

{#if pressable}
  <a
    data-bar={cardId}
    {href}
    aria-label={name}
    class={[
      'flex h-5.5 min-w-0 items-center gap-1 rounded-sm bg-foreground/10 px-1.5 text-xs text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring can-hover:hover:bg-foreground/15',
      fromBefore && 'rounded-l-none',
      toAfter && 'rounded-r-none',
    ]}
    onclick={open}
  >
    {@render content()}
  </a>
{:else}
  <span
    data-bar={cardId}
    aria-hidden="true"
    class={[
      'flex h-5.5 min-w-0 items-center gap-1 rounded-sm bg-foreground/10 px-1.5 text-xs text-foreground',
      fromBefore && 'rounded-l-none',
      toAfter && 'rounded-r-none',
    ]}
  >
    {@render content()}
  </span>
{/if}
