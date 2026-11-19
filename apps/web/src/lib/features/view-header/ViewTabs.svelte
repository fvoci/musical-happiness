<!--
  The views of a board as tabs, where the header has a whole row: each a link
  to the view's address, the current one marked.

  Each tab keeps its whole name. As many as fit are tabs, in their order; the
  rest are lines of a menu behind one more tab ("More views: 2"), the last in
  the row's landmark. The view that is shown is always a tab: where the room
  ends before it, it takes the place of the last one that fits (`tab-fit.ts`).

  What fits is measured: an unseen copy of every tab, and of the button of the
  rest, at its own width; and the room this part of the header has. Both are
  measured again when they change (a view renamed, a font loaded, the pane
  wider).

  "+" adds a view of a kind the reader picks.
-->
<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import type { Attachment } from 'svelte/attachments';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardView } from '$lib/logic/wire.ts';

  import { fittingTabs } from './tab-fit.ts';
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
    /** The view whose tab is to take the focus once it is there; told by `onFocused`. */
    focusId?: string;
    onFocused?: () => void;
  }

  let { views, currentId, viewHref, onOpenView, onAddView, focusId, onFocused }: Props = $props();

  /** For `{@attach}` on the tab of `viewId`: it takes the focus when asked to. */
  function focusWhenAsked(viewId: string): Attachment<HTMLElement> {
    return (tab) => {
      if (focusId !== viewId) return;
      tab.focus();
      onFocused?.();
    };
  }

  /** A tab and the button of the rest look alike: the copies that are measured have these too. */
  const TAB =
    'flex items-center gap-1.5 border-b-2 px-2 text-sm font-medium whitespace-nowrap outline-none focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset';

  /** The width of each tab at its whole name, in the order of `views`. */
  let widths = $state.raw<number[]>([]);
  /** The width of the button of the rest. */
  let moreWidth = $state(0);
  /** The width the tabs and the button of the rest may take. */
  let room = $state(0);

  /**
   * Measures the copies and the room, and again whenever a box resizes or a
   * copy changes (a view renamed, added, of another kind). For `{@attach}` on
   * this part of the header.
   */
  function measured(part: HTMLElement): () => void {
    const copies = part.querySelector<HTMLElement>('[data-copies]');
    const adder = (): HTMLElement | null => part.querySelector<HTMLElement>('[data-adder]');
    const measure = (): void => {
      if (!copies) return;
      const boxes = [...copies.children].map((each) => each.getBoundingClientRect().width);
      moreWidth = boxes.pop() ?? 0;
      widths = boxes;
      room = part.clientWidth - (adder()?.offsetWidth ?? 0);
    };
    measure();
    // Not every environment lays out (and so resizes) elements.
    const resized = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(measure);
    for (const box of [part, copies, adder()]) if (box) resized?.observe(box);
    const changed = new MutationObserver(measure);
    if (copies) changed.observe(copies, { childList: true, subtree: true, characterData: true });
    // "+" comes and goes with the right to add views.
    changed.observe(part, { childList: true });
    return () => {
      resized?.disconnect();
      changed.disconnect();
    };
  }

  const currentIndex = $derived(views.findIndex((each) => each.id === currentId));
  const shown = $derived(
    new Set(
      // Until the copies of views just added or taken away are measured, all of them.
      (widths.length === views.length
        ? fittingTabs(widths, room, moreWidth, currentIndex)
        : views.map((_, index) => index)
      ).map((index) => views[index]?.id),
    ),
  );
  const rest = $derived(views.filter((each) => !shown.has(each.id)));

  const restActions = $derived<MenuAction[]>(
    rest.map((view) => ({
      id: view.id,
      label: viewTitle(view),
      icon: VIEW_ICONS[view.fields.viewType],
      run: () => {
        onOpenView(view.id);
      },
    })),
  );

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

<div {@attach measured} class="relative flex min-w-0 flex-1 items-stretch self-stretch">
  <nav
    aria-label={m.viewheader_views()}
    data-testid="view-tabs"
    class="flex min-w-0 items-stretch overflow-hidden"
  >
    {#each views as each (each.id)}
      {#if shown.has(each.id)}
        {@const Icon = VIEW_ICONS[each.fields.viewType]}
        {@const current = each.id === currentId}
        <a
          href={viewHref(each.id)}
          aria-current={current ? 'page' : undefined}
          title={viewTitle(each)}
          data-view={each.id}
          {@attach focusWhenAsked(each.id)}
          class={[
            TAB,
            current
              ? 'min-w-0 shrink border-foreground text-foreground'
              : 'shrink-0 border-transparent text-muted-foreground hover:text-foreground',
          ]}
          onclick={(event) => {
            follow(event, each.id);
          }}
        >
          <Icon class="size-4 shrink-0" />
          <span class="truncate">{viewTitle(each)}</span>
        </a>
      {/if}
    {/each}

    <!-- In the row's landmark, so that a reader going by landmarks finds every view. -->
    {#if rest.length > 0}
      <ActionMenu title={m.viewheader_views()} actions={restActions} align="start">
        {#snippet trigger(props: Record<string, unknown>)}
          <button
            {...props}
            type="button"
            data-tool="more-views"
            class={[TAB, 'shrink-0 border-transparent text-muted-foreground hover:text-foreground']}
          >
            {m.viewheader_more_views({ count: rest.length })}
            <ChevronDownIcon class="size-4 shrink-0" aria-hidden="true" />
          </button>
        {/snippet}
      </ActionMenu>
    {/if}
  </nav>

  {#if onAddView}
    <span data-adder class="flex shrink-0 items-center">
      <ActionMenu title={m['View.AddView']()} actions={addActions} align="start">
        {#snippet trigger(props: Record<string, unknown>)}
          <IconButton {...props} label={m['View.AddView']()} data-tool="add-view">
            <PlusIcon />
          </IconButton>
        {/snippet}
      </ActionMenu>
    </span>
  {/if}

  <!--
    The copies that are measured: every tab at its whole name, and the button
    of the rest with the most it can count. Out of sight and out of reach,
    in a box of no size, so that their width adds no scrolling to the row.
  -->
  <div
    aria-hidden="true"
    inert
    class="pointer-events-none invisible absolute size-0 overflow-hidden"
  >
    <div data-copies class="flex w-max">
      {#each views as each (each.id)}
        {@const Icon = VIEW_ICONS[each.fields.viewType]}
        <span class={TAB}>
          <Icon class="size-4 shrink-0" />
          <span>{viewTitle(each)}</span>
        </span>
      {/each}
      <span class={TAB}>
        {m.viewheader_more_views({ count: Math.max(views.length - 1, 1) })}
        <ChevronDownIcon class="size-4 shrink-0" />
      </span>
    </div>
  </div>
</div>
