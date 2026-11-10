<!--
  The header of a view: which view, and what is done to it (search, filter,
  sort, group, properties, new card).

  Where the pane it is in has room for it (`pane.header`, 62rem) everything
  is in one row, with words. The pane, not the window: next to a fixed
  sidebar or an open card the same window has a narrower pane. Below that
  it folds, in one of two ways:

  - `a`: one row. The view tabs become one button that names the current view;
    search and filter stay as icons; the rest is behind "…". "New" leaves the
    header for a round button at the bottom of the screen (drawn by the shell).
  - `b`: two rows. The view tabs keep their row and scroll sideways; the
    second row has every tool as an icon and "New" as a button with its word.
-->
<script lang="ts">
  import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
  import CalendarDaysIcon from '@lucide/svelte/icons/calendar-days';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import EyeIcon from '@lucide/svelte/icons/eye';
  import LayersIcon from '@lucide/svelte/icons/layers';
  import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
  import ListFilterIcon from '@lucide/svelte/icons/list-filter';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SquareKanbanIcon from '@lucide/svelte/icons/square-kanban';
  import TableIcon from '@lucide/svelte/icons/table-2';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { untrack } from 'svelte';

  import { Button } from '$lib/ui/button/index.js';

  import ActionMenu from './ActionMenu.svelte';
  import type { Action, IconComponent } from './actions.ts';
  import { FILTERS, GROUPABLE, SORTS, VIEWS, type ViewKind, type ViewLink } from './data.ts';
  import IconButton from './IconButton.svelte';
  import { pane } from './layout.svelte.ts';
  import Surface from './Surface.svelte';
  import { T } from './text.ts';
  import ViewSettings from './ViewSettings.svelte';

  interface Props {
    folding: 'a' | 'b';
    view: ViewLink;
    /** What is open when the page loads. */
    opened: 'none' | 'settings' | 'more' | 'views';
    /** Menus are bottom sheets. */
    sheet: boolean;
  }

  let { folding, view, opened, sheet }: Props = $props();

  let settingsOpen = $state(untrack(() => opened === 'settings'));
  let moreOpen = $state(untrack(() => opened === 'more'));
  let viewsOpen = $state(untrack(() => opened === 'views'));

  const VIEW_ICONS: Record<ViewKind, IconComponent> = {
    kanban: SquareKanbanIcon,
    table: TableIcon,
    gallery: LayoutGridIcon,
    calendar: CalendarDaysIcon,
  };

  const ViewIcon = $derived(VIEW_ICONS[view.kind]);

  const viewActions = $derived<Action[]>([
    ...VIEWS.map((entry) => ({
      id: entry.id,
      label: entry.name,
      icon: VIEW_ICONS[entry.kind],
      checked: entry.id === view.id,
    })),
    { id: 'add', label: T.addView, icon: PlusIcon, separated: true },
  ]);

  const VIEW_COMMANDS: Action[] = [
    { id: 'rename', label: T.renameView, icon: PencilIcon, separated: true },
    { id: 'duplicate', label: T.duplicateView, icon: CopyIcon },
    { id: 'export', label: T.exportCsv, icon: DownloadIcon },
    { id: 'delete', label: T.deleteView, icon: TrashIcon, destructive: true, separated: true },
  ];
  const PROPERTIES: Action = { id: 'properties', label: T.properties, icon: EyeIcon, hint: '4개' };

  /** Behind "…": whatever of the tools has no button of its own in this arrangement. */
  const moreActions = $derived.by<Action[]>(() => {
    if (pane.header) return VIEW_COMMANDS.map((action, at) => ({ ...action, separated: at === 3 }));
    if (folding === 'b') return [PROPERTIES, ...VIEW_COMMANDS];
    return [
      {
        id: 'sort',
        label: T.sort,
        icon: ArrowUpDownIcon,
        hint: `${SORTS[0].property} 외 ${SORTS.length - 1}`,
      },
      { id: 'group', label: T.groupBy, icon: LayersIcon, hint: GROUPABLE[0] },
      PROPERTIES,
      ...VIEW_COMMANDS,
    ];
  });
</script>

{#snippet filterCount()}
  <span
    class="inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-full bg-primary px-1 text-[0.6875rem] leading-none font-semibold text-primary-foreground"
  >
    {FILTERS.length}
  </span>
{/snippet}

<!-- The filter button and what it opens. With words where the header is whole, an icon and a count otherwise. -->
{#snippet settings()}
  <Surface
    bind:open={settingsOpen}
    {sheet}
    title={sheet ? T.viewSettings : T.filter}
    subtitle={sheet ? view.name : undefined}
    popoverClass="w-[32rem]"
  >
    {#snippet trigger(props: Record<string, unknown>)}
      {#if pane.header}
        <Button {...props} variant="ghost">
          <ListFilterIcon data-icon="inline-start" />
          {T.filter}
          {@render filterCount()}
        </Button>
      {:else}
        <Button
          {...props}
          variant="ghost"
          aria-label="{T.filter} ({FILTERS.length})"
          class="gap-1 px-2 pointer-coarse:h-touch pointer-coarse:min-w-touch"
        >
          <ListFilterIcon />
          {@render filterCount()}
        </Button>
      {/if}
    {/snippet}
    <ViewSettings tabs={sheet} />
  </Surface>
{/snippet}

{#snippet more()}
  <ActionMenu
    bind:open={moreOpen}
    {sheet}
    title={T.viewMenu}
    subtitle={view.name}
    actions={moreActions}
  >
    {#snippet trigger(props: Record<string, unknown>)}
      <IconButton {...props} label={T.viewMenu}><EllipsisIcon /></IconButton>
    {/snippet}
  </ActionMenu>
{/snippet}

<!-- The views as tabs. The current one is underlined. -->
{#snippet tabs()}
  <nav
    aria-label={T.views}
    class="no-scrollbar flex min-w-0 items-stretch self-stretch overflow-x-auto"
  >
    {#each VIEWS as entry (entry.id)}
      {@const Icon = VIEW_ICONS[entry.kind]}
      <button
        type="button"
        aria-current={entry.id === view.id ? 'page' : undefined}
        class={[
          'flex shrink-0 items-center gap-1.5 border-b-2 px-2.5 text-sm font-medium whitespace-nowrap',
          entry.id === view.id
            ? 'border-foreground text-foreground'
            : 'border-transparent text-muted-foreground hover:text-foreground',
        ]}
      >
        <Icon class="size-4" />
        {entry.name}
      </button>
    {/each}
    <IconButton label={T.addView} class="shrink-0 self-center"><PlusIcon /></IconButton>
  </nav>
{/snippet}

{#if pane.header}
  <div data-testid="view-header" class="flex h-12 shrink-0 items-center gap-1 border-b px-4">
    {@render tabs()}
    <div class="flex-1"></div>
    <Button variant="ghost">
      <EyeIcon data-icon="inline-start" />
      {T.properties}
    </Button>
    <Button variant="ghost">
      <LayersIcon data-icon="inline-start" />
      {T.group}: {GROUPABLE[0]}
    </Button>
    {@render settings()}
    <Button variant="ghost">
      <ArrowUpDownIcon data-icon="inline-start" />
      {T.sort}
    </Button>
    <IconButton label={T.search}><SearchIcon /></IconButton>
    {@render more()}
    <Button class="ml-1">
      <PlusIcon data-icon="inline-start" />
      {T.newCard}
    </Button>
  </div>
{:else if folding === 'a'}
  <div data-testid="view-header" class="flex h-12 shrink-0 items-center gap-0.5 border-b px-1.5">
    <ActionMenu bind:open={viewsOpen} {sheet} title={T.views} actions={viewActions} align="start">
      {#snippet trigger(props: Record<string, unknown>)}
        <Button {...props} variant="ghost" class="min-w-0 shrink pointer-coarse:h-touch">
          <ViewIcon data-icon="inline-start" />
          <span class="truncate">{view.name}</span>
          <ChevronDownIcon data-icon="inline-end" class="text-muted-foreground" />
        </Button>
      {/snippet}
    </ActionMenu>
    <div class="flex-1"></div>
    <IconButton label={T.search}><SearchIcon /></IconButton>
    {@render settings()}
    {@render more()}
  </div>
{:else}
  <div data-testid="view-header" class="shrink-0 border-b">
    <div class="flex h-9 items-center px-1.5 pointer-coarse:h-touch">
      {@render tabs()}
    </div>
    <div data-testid="view-tools" class="flex h-12 items-center gap-0.5 border-t px-1.5">
      <IconButton label={T.search}><SearchIcon /></IconButton>
      {@render settings()}
      <IconButton label={T.sort}><ArrowUpDownIcon /></IconButton>
      <IconButton label={T.groupBy}><LayersIcon /></IconButton>
      {@render more()}
      <div class="flex-1"></div>
      <!--
        Five targets of a finger and the full word for "new" do not fit one
        row of a screen 360 wide. The word is the shorter one here.
      -->
      <Button aria-label={T.newCard} class="pointer-coarse:h-touch pointer-coarse:px-3">
        <PlusIcon data-icon="inline-start" />
        {T.newCardShort}
      </Button>
    </div>
  </div>
{/if}
