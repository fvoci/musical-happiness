<!--
  One card of the chart: its title at the left, which stays there while the
  days scroll, and its bar on its days.

  The bar is a button that opens the card. The bars are one stop of the Tab
  key, and the arrow keys walk them (`onkeys`); `current` is the one the stop
  is on. The title is a link to the card for a pointer: it is out of the
  order of the Tab key, which has the bar. A bar may be a few pixels wide:
  for a finger it takes presses from a little around it, and the title is
  the wide thing to press.

  A card that starts before a card it comes after has ended carries a
  warning mark, and one on a loop a mark of going round: next to its title,
  where they stay in sight while the days scroll (next to the bar they would
  lie on the arrows that leave it). The marks are for the eye; the name of
  the bar says the same in words.
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
    /** The card is open and was opened from the title: the focus comes back to the title. */
    viaTitle: boolean;
    /** The reader was sent to this bar: its ring shows however the focus came. */
    pointed: boolean;
    onopen: (cardId: string) => void;
    /** The title was pressed. */
    onopentitle: (cardId: string) => void;
    onkeys: (event: KeyboardEvent, cardId: string) => void;
    /** The bar took the focus: the stop of the Tab key is on it from now on. */
    onfocused: (cardId: string, bar: HTMLElement) => void;
  }

  let { line, current, href, viaTitle, pointed, onopen, onopentitle, onkeys, onfocused }: Props =
    $props();

  /** The bar is wide enough for a few letters of the title; a narrower one is a plain block. */
  const labelled = $derived(line.bar.width >= LABEL_MIN_WIDTH);

  function follow(event: MouseEvent): void {
    // A new tab or window is the browser's to open.
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
      return;
    event.preventDefault();
    onopentitle(line.id);
  }
</script>

<li
  data-row={line.id}
  class="relative z-1 flex h-(--gantt-row) border-b border-border/60 can-hover:hover:bg-muted/40"
>
  <div
    class="sticky left-0 z-10 flex w-(--gantt-title) shrink-0 items-stretch border-r bg-background"
  >
    <a
      {href}
      tabindex="-1"
      data-row-title
      class="flex min-w-0 flex-1 items-center px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset can-hover:hover:bg-muted"
      onclick={follow}
    >
      <span class="truncate">{line.title}</span>
      {#if line.late || line.loop}
        <span
          aria-hidden="true"
          class="ml-auto flex shrink-0 items-center gap-0.5 pl-1 text-destructive"
        >
          {#if line.late}<TriangleAlertIcon data-mark="late" class="size-3.5" />{/if}
          {#if line.loop}<RepeatIcon data-mark="loop" class="size-3.5" />{/if}
        </span>
      {/if}
    </a>
  </div>
  <div class="relative min-w-(--gantt-width) flex-1">
    <button
      type="button"
      data-bar={line.id}
      data-card={viaTitle ? undefined : line.id}
      data-late={line.late ? '' : undefined}
      data-loop={line.loop ? '' : undefined}
      tabindex={current ? 0 : -1}
      aria-label={line.name}
      title={line.name}
      class={[
        'absolute top-1/2 flex h-6 -translate-y-1/2 items-center gap-1 rounded-sm bg-primary/80 text-left text-xs text-primary-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background can-hover:hover:bg-primary roomy:h-8 roomy:after:absolute roomy:after:-inset-x-3 roomy:after:-inset-y-1.5',
        labelled && 'px-1.5',
        pointed && 'focus:ring-2 focus:ring-ring focus:ring-offset-1 focus:ring-offset-background',
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
      onfocus={(event) => {
        onfocused(line.id, event.currentTarget);
      }}
    >
      {#if line.bar.fromBefore && labelled}
        <ChevronLeftIcon aria-hidden="true" class="size-3 shrink-0 opacity-70" />
      {/if}
      {#if labelled}<span class="min-w-0 truncate">{line.title}</span>{/if}
      {#if line.bar.toAfter && labelled}
        <ChevronRightIcon aria-hidden="true" class="ml-auto size-3 shrink-0 opacity-70" />
      {/if}
    </button>
  </div>
</li>
