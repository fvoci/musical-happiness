<!--
  A page with its outline beside it, at its right, where the page's column
  has room for both: the page keeps its width (in the middle while the room
  allows, moved to the left only as far as the outline needs), and the
  outline stays in sight as the page scrolls, no taller than what shows of
  the column. Its head folds it away (`PageOutline.setFolded`), the focus
  going to the button that puts it back.

  The page is drawn in the same place whether the outline is beside it or
  not: an editor in it is not made anew when a heading is added or the
  outline is folded.
-->
<script lang="ts">
  import PanelRightCloseIcon from '@lucide/svelte/icons/panel-right-close';
  import { tick, type Snippet } from 'svelte';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';

  import OutlineNav from './OutlineNav.svelte';
  import type { PageOutline } from './page-outline.svelte.ts';

  interface Props {
    outline: PageOutline;
    /** The identifier of the outline; the button that puts it back is told it (`OutlineButton`). */
    id: string;
    children: Snippet;
  }

  let { outline, id, children }: Props = $props();

  const beside = $derived(outline.mode === 'side');

  async function fold(): Promise<void> {
    outline.setFolded(true);
    await tick();
    document.getElementById(`${id}-button`)?.focus();
  }
</script>

<div
  class={[beside && 'grid']}
  style:grid-template-columns={beside
    ? `minmax(0, 1fr) ${outline.page}rem minmax(${outline.side}rem, 1fr)`
    : undefined}
>
  {#if beside}
    <div
      class="sticky top-0 col-start-3 row-start-1 flex min-h-0 max-w-64 flex-col self-start pt-6 pr-4 pb-4"
      style:max-height="{outline.height}px"
      data-testid="page-outline-beside"
    >
      <OutlineNav {outline} {id} class="flex-1">
        {#snippet head()}
          <div class="flex shrink-0 items-center gap-1 pb-1 pl-2.5">
            <p class="min-w-0 flex-1 truncate text-xs font-medium text-muted-foreground">
              {m.wiki_outline()}
            </p>
            <IconButton
              label={m.wiki_outline_fold()}
              class="text-muted-foreground"
              data-testid="fold-page-outline"
              onclick={() => void fold()}
            >
              <PanelRightCloseIcon />
            </IconButton>
          </div>
        {/snippet}
      </OutlineNav>
    </div>
  {/if}
  <div class={[beside && 'col-start-2 row-start-1 min-w-0']}>
    {@render children()}
  </div>
</div>
