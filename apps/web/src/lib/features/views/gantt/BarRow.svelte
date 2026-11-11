<!--
  One card of the chart: its title at the left, which stays there while the
  days scroll, and its bar on its days.

  The bar is a button that opens the card. The bars are one stop of the Tab
  key, and the arrow keys walk them (`onkeys`); `current` is the one the stop
  is on. The title is a link to the card for a pointer: it is out of the
  order of the Tab key, which has the bar.

  A card that starts before a card it comes after has ended carries a
  warning mark, and one on a loop a mark of going round: next to its title,
  where they stay in sight, and after its bar. The marks are for the eye;
  the name of the bar says the same in words.

  A row that is out of view is not laid out or painted until it comes near
  (`content-visibility`), as in the table: with thousands of cards that is
  most of the work of drawing the chart and of scrolling it.
-->
<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import RepeatIcon from '@lucide/svelte/icons/repeat';
  import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';

  import type { CardLine } from './chart.svelte.ts';

  /** In rem. */
  const LABEL_MIN_WIDTH = 3;

  interface Props {
    line: CardLine;
    /** The Tab key stops on this bar. */
    current: boolean;
    href: string;
    onopen: (cardId: string) => void;
    onkeys: (event: KeyboardEvent, cardId: string) => void;
    /** The bar took the focus: the stop of the Tab key is on it from now on. */
    onfocused: (cardId: string) => void;
  }

  let { line, current, href, onopen, onkeys, onfocused }: Props = $props();

  /** The bar is wide enough for a few letters of the title; a narrower one is a plain block. */
  const labelled = $derived(line.bar.width >= LABEL_MIN_WIDTH);

  function follow(event: MouseEvent): void {
    // A new tab or window is the browser's to open.
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
      return;
    event.preventDefault();
    onopen(line.id);
  }
</script>

{#snippet marks(place: string)}
  {#if line.late || line.loop}
    <span aria-hidden="true" class={['flex shrink-0 items-center gap-0.5 text-destructive', place]}>
      {#if line.late}<TriangleAlertIcon data-mark="late" class="size-3.5" />{/if}
      {#if line.loop}<RepeatIcon data-mark="loop" class="size-3.5" />{/if}
    </span>
  {/if}
{/snippet}

<li
  data-row={line.id}
  class="relative z-1 flex h-(--gantt-row) border-b border-border/60 [contain-intrinsic-block-size:auto_var(--gantt-row)] [content-visibility:auto] can-hover:hover:bg-muted/40"
>
  <div
    class="sticky left-0 z-10 flex w-(--gantt-title) shrink-0 items-stretch border-r bg-background"
  >
    <a
      {href}
      tabindex="-1"
      data-row-title
      class="flex min-w-0 flex-1 items-center px-3 outline-none can-hover:hover:bg-muted"
      onclick={follow}
    >
      <span class="truncate">{line.title}</span>
      {@render marks('ml-auto pl-1')}
    </a>
  </div>
  <div class="relative min-w-(--gantt-width) flex-1">
    <button
      type="button"
      data-bar={line.id}
      data-card={line.id}
      data-late={line.late ? '' : undefined}
      data-loop={line.loop ? '' : undefined}
      tabindex={current ? 0 : -1}
      aria-label={line.name}
      title={line.name}
      class={[
        'absolute top-1/2 flex h-6 -translate-y-1/2 items-center gap-1 overflow-hidden rounded-sm bg-foreground/20 text-left text-xs text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background can-hover:hover:bg-foreground/30 roomy:h-8',
        labelled && 'px-1.5',
        line.bar.fromBefore && 'rounded-l-none',
        line.bar.toAfter && 'rounded-r-none',
      ]}
      style:left="{line.bar.x}rem"
      style:width="{line.bar.width}rem"
      onclick={() => {
        onopen(line.id);
      }}
      onkeydown={(event) => {
        onkeys(event, line.id);
      }}
      onfocus={() => {
        onfocused(line.id);
      }}
    >
      {#if line.bar.fromBefore}
        <ChevronLeftIcon aria-hidden="true" class="size-3 shrink-0 opacity-60" />
      {/if}
      {#if labelled}<span class="min-w-0 truncate">{line.title}</span>{/if}
      {#if line.bar.toAfter}
        <ChevronRightIcon aria-hidden="true" class="ml-auto size-3 shrink-0 opacity-60" />
      {/if}
    </button>
    <div
      class="pointer-events-none absolute top-1/2 -translate-y-1/2 pl-1"
      style:left="{line.bar.x + line.bar.width}rem"
    >
      {@render marks('')}
    </div>
  </div>
</li>
