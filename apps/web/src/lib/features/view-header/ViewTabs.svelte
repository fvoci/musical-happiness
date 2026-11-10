<!--
  The views of a board as tabs, where the header has a whole row: each a link
  to the view's address, the current one marked. With many views the tabs of
  the others narrow before the row starts to scroll; the current one keeps
  its whole name and is scrolled into sight.

  "+" adds a view of a kind the reader picks.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardView } from '$lib/logic/wire.ts';

  import { VIEW_ICONS, VIEW_KIND_NAMES, viewTitle } from './texts.ts';
  import { ADDABLE_VIEW_TYPES, type AddableViewType } from './view-fields.ts';

  interface Props {
    /** In the order of the tabs. */
    views: readonly BoardView[];
    currentId: string;
    viewHref: (viewId: string) => string;
    onOpenView: (viewId: string) => void;
    /** Without it there is no "+": the reader may not add views. */
    onAddView?: (viewType: AddableViewType) => void;
  }

  let { views, currentId, viewHref, onOpenView, onAddView }: Props = $props();

  let nav = $state<HTMLElement | null>(null);

  // The tab of the view that is shown is in sight, also after its name or the row changed.
  $effect(() => {
    // Read here so that the effect runs again when the view or the row of tabs changes.
    if (!views.some((each) => each.id === currentId)) return;
    const current = nav?.querySelector<HTMLElement>('[aria-current="page"]');
    // Not every environment lays out (and so scrolls) elements.
    if (typeof current?.scrollIntoView === 'function') {
      current.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }
  });

  const addActions = $derived<MenuAction[]>(
    ADDABLE_VIEW_TYPES.map((viewType) => ({
      id: `add-${viewType}`,
      label: VIEW_KIND_NAMES[viewType](),
      icon: VIEW_ICONS[viewType],
      run: () => onAddView?.(viewType),
    })),
  );

  function follow(event: MouseEvent, viewId: string): void {
    // A click that asks for a new tab or window is the browser's.
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    onOpenView(viewId);
  }
</script>

<nav
  bind:this={nav}
  aria-label={m.viewheader_views()}
  data-testid="view-tabs"
  class="flex min-w-0 [scrollbar-width:none] items-stretch self-stretch overflow-x-auto"
>
  {#each views as each (each.id)}
    {@const Icon = VIEW_ICONS[each.fields.viewType]}
    {@const current = each.id === currentId}
    <a
      href={viewHref(each.id)}
      aria-current={current ? 'page' : undefined}
      title={viewTitle(each)}
      data-view={each.id}
      class={[
        'flex items-center gap-1.5 border-b-2 px-2 text-sm font-medium whitespace-nowrap outline-none focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset',
        current
          ? 'shrink-0 border-foreground text-foreground'
          : 'min-w-16 shrink border-transparent text-muted-foreground hover:text-foreground',
      ]}
      onclick={(event) => {
        follow(event, each.id);
      }}
    >
      <Icon class="size-4 shrink-0" />
      <span class="truncate">{viewTitle(each)}</span>
    </a>
  {/each}
</nav>
{#if onAddView}
  <ActionMenu title={m['View.AddView']()} actions={addActions} align="start">
    {#snippet trigger(props: Record<string, unknown>)}
      <IconButton {...props} label={m['View.AddView']()} data-tool="add-view" class="shrink-0">
        <PlusIcon />
      </IconButton>
    {/snippet}
  </ActionMenu>
{/if}
