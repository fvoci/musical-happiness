<!--
  The header of the calendar view: which view, and what is done to it. It
  folds the way the mobile proposals settled for every view (one row; the
  views behind one button, the rest behind "…") where the pane is narrower
  than 62rem.

  A calendar has no grouping and no sorting. In their place it has "display
  by": the date property whose value puts a card on a day. Where the header
  is whole that is a button of its own; folded, it is a line of the "…" menu
  that leads to the list of properties (a second page of the same sheet or
  popover, with a way back). The same menu has "month" and "week" where the
  row above the grid has no room for that switch.
-->
<script lang="ts">
  import CalendarDaysIcon from '@lucide/svelte/icons/calendar-days';
  import CalendarRangeIcon from '@lucide/svelte/icons/calendar-range';
  import CheckIcon from '@lucide/svelte/icons/check';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import EyeIcon from '@lucide/svelte/icons/eye';
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

  import type { IconComponent } from '../mobile/actions.ts';
  import { VIEWS, type ViewKind } from '../mobile/data.ts';
  import IconButton from '../mobile/IconButton.svelte';
  import { pane } from '../mobile/layout.svelte.ts';
  import Surface from '../mobile/Surface.svelte';
  import { T as MOBILE } from '../mobile/text.ts';

  import { DATE_PROPERTIES, DISPLAY_BY } from './data.ts';
  import { T } from './text.ts';
  import { view, type Mode } from './view.svelte.ts';

  interface Props {
    /** Menus are bottom sheets. */
    sheet: boolean;
    /** The list of date properties is open when the page loads. */
    byOpen: boolean;
  }

  let { sheet, byOpen }: Props = $props();

  /** Where the header is whole, "display by" has a popover of its own. */
  let propertiesOpen = $state(untrack(() => byOpen && pane.header));
  let moreOpen = $state(untrack(() => byOpen && !pane.header));
  let page = $state<'commands' | 'by'>(untrack(() => (byOpen ? 'by' : 'commands')));

  const VIEW_ICONS: Record<ViewKind, IconComponent> = {
    kanban: SquareKanbanIcon,
    table: TableIcon,
    gallery: LayoutGridIcon,
    calendar: CalendarDaysIcon,
  };
  const current = VIEWS.find((entry) => entry.kind === 'calendar');

  const COMMANDS: { id: string; label: string; icon: IconComponent; destructive?: boolean }[] = [
    { id: 'rename', label: MOBILE.renameView, icon: PencilIcon },
    { id: 'duplicate', label: MOBILE.duplicateView, icon: CopyIcon },
    { id: 'export', label: MOBILE.exportCsv, icon: DownloadIcon },
    { id: 'delete', label: MOBILE.deleteView, icon: TrashIcon, destructive: true },
  ];
  const MODES: { mode: Mode; label: string }[] = [
    { mode: 'month', label: T.monthView },
    { mode: 'week', label: T.weekView },
  ];

  const LINE =
    'flex min-h-9 w-full items-center gap-3 rounded-lg px-3 text-left hover:bg-muted pointer-coarse:min-h-12 pointer-coarse:text-base';

  function setMode(mode: Mode): void {
    view.mode = mode;
    moreOpen = false;
  }
</script>

<!-- The date properties of the board. One is in use; two cannot be changed by moving a card. -->
{#snippet properties()}
  <ul data-testid="display-by" class="px-2 pb-2">
    {#each DATE_PROPERTIES as property (property.id)}
      <li>
        <button type="button" aria-pressed={property.id === DISPLAY_BY.id} class={LINE}>
          <span class="flex min-w-0 flex-1 flex-col">
            <span class="truncate">{property.name}</span>
            {#if property.readOnly}
              <span class="truncate text-xs text-muted-foreground">{T.readOnly}</span>
            {/if}
          </span>
          {#if property.id === DISPLAY_BY.id}<CheckIcon class="size-5 shrink-0" />{/if}
        </button>
      </li>
    {/each}
  </ul>
{/snippet}

{#snippet menu()}
  {#if page === 'by'}
    <div class="px-2 pt-1">
      <button
        type="button"
        class={[LINE, 'font-medium']}
        onclick={() => {
          page = 'commands';
        }}
      >
        <ChevronLeftIcon class="size-5 shrink-0" />
        {MOBILE.viewMenu}
      </button>
    </div>
    {@render properties()}
  {:else}
    <ul data-testid="view-menu" class="px-2 pt-1 pb-2">
      <li>
        <button
          type="button"
          class={LINE}
          onclick={() => {
            page = 'by';
          }}
        >
          <CalendarRangeIcon class="size-5 shrink-0" />
          <span class="min-w-0 flex-1 truncate">{T.displayBy}</span>
          <span class="shrink-0 text-sm text-muted-foreground">{DISPLAY_BY.name}</span>
          <ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
        </button>
      </li>
      <!-- Where the row above the grid has no room for the switch. A menu is outside of the pane, so script asks. -->
      {#if !pane.columns}
        {#each MODES as { mode, label } (mode)}
          <li>
            <button
              type="button"
              aria-pressed={view.mode === mode}
              class={LINE}
              onclick={() => {
                setMode(mode);
              }}
            >
              <CalendarDaysIcon class="size-5 shrink-0" />
              <span class="min-w-0 flex-1 truncate">{label}</span>
              {#if view.mode === mode}<CheckIcon class="size-5 shrink-0" />{/if}
            </button>
          </li>
        {/each}
      {/if}
      <li>
        <button type="button" class={LINE}>
          <EyeIcon class="size-5 shrink-0" />
          <span class="min-w-0 flex-1 truncate">{MOBILE.properties}</span>
        </button>
      </li>
      {#each COMMANDS as command (command.id)}
        {@const Icon = command.icon}
        <li class={[(command.id === 'rename' || command.destructive) && 'mt-1 border-t pt-1']}>
          <button type="button" class={[LINE, command.destructive && 'text-destructive']}>
            <Icon class="size-5 shrink-0" />
            <span class="min-w-0 flex-1 truncate">{command.label}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

{#snippet more()}
  <Surface
    bind:open={moreOpen}
    {sheet}
    title={page === 'by' ? T.displayBy : MOBILE.viewMenu}
    subtitle={current?.name}
    popoverClass="w-72"
  >
    {#snippet trigger(props: Record<string, unknown>)}
      <IconButton {...props} label={MOBILE.viewMenu} data-testid="view-more">
        <EllipsisIcon />
      </IconButton>
    {/snippet}
    {@render menu()}
  </Surface>
{/snippet}

{#if pane.header}
  <div data-testid="view-header" class="flex h-12 shrink-0 items-center gap-1 border-b px-4">
    <nav aria-label={MOBILE.views} class="flex min-w-0 items-stretch self-stretch">
      {#each VIEWS as entry (entry.id)}
        {@const Icon = VIEW_ICONS[entry.kind]}
        <button
          type="button"
          aria-current={entry.kind === 'calendar' ? 'page' : undefined}
          class={[
            'flex shrink-0 items-center gap-1.5 border-b-2 px-2.5 text-sm font-medium whitespace-nowrap',
            entry.kind === 'calendar'
              ? 'border-foreground text-foreground'
              : 'border-transparent text-muted-foreground hover:text-foreground',
          ]}
        >
          <Icon class="size-4" />
          {entry.name}
        </button>
      {/each}
      <IconButton label={MOBILE.addView} class="shrink-0 self-center"><PlusIcon /></IconButton>
    </nav>
    <div class="flex-1"></div>
    <Button variant="ghost">
      <EyeIcon data-icon="inline-start" />
      {MOBILE.properties}
    </Button>
    <Surface bind:open={propertiesOpen} sheet={false} title={T.displayBy} popoverClass="w-64 pt-2">
      {#snippet trigger(props: Record<string, unknown>)}
        <Button {...props} variant="ghost" data-testid="display-by-button">
          <CalendarRangeIcon data-icon="inline-start" />
          {T.displayBy}: {DISPLAY_BY.name}
        </Button>
      {/snippet}
      {@render properties()}
    </Surface>
    <Button variant="ghost">
      <ListFilterIcon data-icon="inline-start" />
      {MOBILE.filter}
    </Button>
    <IconButton label={MOBILE.search}><SearchIcon /></IconButton>
    {@render more()}
    <Button class="ml-1">
      <PlusIcon data-icon="inline-start" />
      {MOBILE.newCard}
    </Button>
  </div>
{:else}
  <div data-testid="view-header" class="flex h-12 shrink-0 items-center gap-0.5 border-b px-1.5">
    <Button variant="ghost" class="min-w-0 shrink pointer-coarse:h-touch">
      <CalendarDaysIcon data-icon="inline-start" />
      <span class="truncate">{current?.name}</span>
      <ChevronDownIcon data-icon="inline-end" class="text-muted-foreground" />
    </Button>
    <div class="flex-1"></div>
    <IconButton label={MOBILE.search}><SearchIcon /></IconButton>
    <IconButton label={MOBILE.filter}><ListFilterIcon /></IconButton>
    {@render more()}
  </div>
{/if}
