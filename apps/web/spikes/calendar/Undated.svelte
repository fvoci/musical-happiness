<!--
  The cards that have no value for the property the calendar places cards
  by, and so no place in the grid. Two proposals for where they are:

  - a drawer, opened from a button above the grid that counts them: a bottom
    sheet in a compact window, a column at the right otherwise (`list` is its
    content);
  - a strip under the grid that is always there, folded to one line or open
    (`strip`).

  Either way a card gets a date from "…" on its line, and with a mouse by
  being dragged onto a day.
-->
<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronUpIcon from '@lucide/svelte/icons/chevron-up';
  import XIcon from '@lucide/svelte/icons/x';
  import { untrack } from 'svelte';

  import IconButton from '../mobile/IconButton.svelte';

  import CardRow from './CardRow.svelte';
  import { DISPLAY_BY, UNDATED } from './data.ts';
  import { T } from './text.ts';

  interface Props {
    form: 'list' | 'strip';
    /** `strip`: open when the page loads. */
    open?: boolean;
    /** Menus are bottom sheets. */
    sheet: boolean;
    /** `list`, where it can be closed. */
    onclose?: () => void;
  }

  let { form, open = false, sheet, onclose }: Props = $props();

  let unfolded = $state(untrack(() => open));
</script>

{#snippet rows()}
  <p class="px-3 pb-1 text-xs text-muted-foreground">
    {T.displayBy}: {DISPLAY_BY.name}. {T.undatedHint}
  </p>
  {#each UNDATED as entry (entry.id)}
    <CardRow {entry} {sheet} />
  {/each}
{/snippet}

{#if form === 'list'}
  <section data-testid="undated-list" class="flex min-h-0 flex-1 flex-col">
    {#if onclose}
      <header class="flex shrink-0 items-center gap-2 py-1 pr-1 pl-4">
        <h3 class="font-semibold">{T.undatedCount(UNDATED.length)}</h3>
        <div class="flex-1"></div>
        <IconButton label={T.closeUndated} onclick={onclose}><XIcon /></IconButton>
      </header>
    {/if}
    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1 pb-2">
      {@render rows()}
    </div>
  </section>
{:else}
  <section data-testid="undated-strip" class="shrink-0 border-t bg-background pb-(--safe-bottom)">
    <button
      type="button"
      aria-expanded={unfolded}
      aria-label={unfolded ? T.hideUndated : T.showUndated}
      class="flex h-10 w-full items-center gap-2 px-4 text-left font-medium hover:bg-muted pointer-coarse:h-touch"
      onclick={() => {
        unfolded = !unfolded;
      }}
    >
      {T.undatedCount(UNDATED.length)}
      <span class="flex-1"></span>
      {#if unfolded}
        <ChevronDownIcon class="size-4 text-muted-foreground" />
      {:else}
        <ChevronUpIcon class="size-4 text-muted-foreground" />
      {/if}
    </button>
    {#if unfolded}
      <!-- Lower in a narrow pane: the small grid above it must stay whole. -->
      <div class="max-h-36 overflow-y-auto overscroll-contain px-1 pb-2 @columns/pane:max-h-56">
        {@render rows()}
      </div>
    {/if}
  </section>
{/if}
