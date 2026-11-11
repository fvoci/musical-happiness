<!--
  The header of a view: which view is shown, and what is done to it (which
  properties its cards show, grouping, filter, sort, search, a new card).

  It looks at the width of the pane it is in, not of the window
  (`pane.header`). With room for all of it, it is one row: the views as
  tabs, the tools with their words, the search field, "New". In a narrower
  pane it folds into one row of a different make: one button that names the
  view and opens the list of views, search and filter as icons, the rest
  behind "…", and "New" as a round button at the bottom of the board.

  The two are one component, and every tool is one element in both. A tool
  that has no button of its own (in the folded row all but search and filter;
  "dependencies" in neither) keeps its element, unseen, on "…": what it opened stays
  open while the pane changes its width, and stays next to the button it was
  opened from. Whether a tool opens as a popover or as a sheet is not decided
  here (lib/layout/presentation.ts).

  A reader who may not edit the board can switch views and search. The
  other tools change the view for everyone and are not shown.
-->
<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
  import CalendarDaysIcon from '@lucide/svelte/icons/calendar-days';
  import EyeIcon from '@lucide/svelte/icons/eye';
  import GitForkIcon from '@lucide/svelte/icons/git-fork';
  import LayersIcon from '@lucide/svelte/icons/layers';
  import ListFilterIcon from '@lucide/svelte/icons/list-filter';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SearchIcon from '@lucide/svelte/icons/search';
  import { tick } from 'svelte';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { isComposing } from '$lib/components/keys.ts';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { PickerPresentation } from '$lib/components/picker-surface/default-presentation.svelte.ts';
  import PickerSurface from '$lib/components/picker-surface/PickerSurface.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { pane } from '$lib/layout/room.svelte.ts';
  import type { BoardView, PropertyTemplate } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';

  import ChoiceTool from './ChoiceTool.svelte';
  import { conditionCount } from './filter-edit.ts';
  import FilterEditor from './FilterEditor.svelte';
  import PropertiesPanel from './PropertiesPanel.svelte';
  import SortEditor from './SortEditor.svelte';
  import { asideTools, NEW_VIEW_TITLES, viewTitle, type Icon } from './texts.ts';
  import {
    dependenciesChoices,
    dependenciesName,
    dependenciesOffered,
    displayByChoices,
    displayByName,
    displayByOffered,
    groupableProperties,
    groupingFields,
    groupingOf,
    groupProperty,
    newViewFields,
    sortOffered,
    type AddableViewType,
    type DisplayByChoice,
  } from './view-fields.ts';
  import ViewMenu from './ViewMenu.svelte';
  import ViewSwitcher from './ViewSwitcher.svelte';
  import ViewTabs from './ViewTabs.svelte';

  interface Props {
    board: BoardStore;
    view: BoardView;
    /** The search text of the view. Bindable. */
    search: string;
    viewHref: (viewId: string) => string;
    onOpenView: (viewId: string) => void;
    /** Makes a card the view will show, and opens it. */
    onAddCard: () => void;
  }

  let { board, view, search = $bindable(), viewHref, onOpenView, onAddCard }: Props = $props();

  const pickers = new PickerPresentation();

  /** The pane has room for the whole header in one row. */
  const row = $derived(pane.header);
  const templates = $derived(board.board?.cardProperties ?? []);
  const canEdit = $derived(board.canEdit);
  const grouping = $derived(groupingOf(view.fields.viewType));
  const sorting = $derived(sortOffered(view.fields.viewType));
  const groupedBy = $derived(groupProperty(view, templates));
  const groupName = $derived(groupedBy?.name ?? m.viewheader_group_none());
  /** A calendar or a Gantt chart: the date its cards are placed by; when they were made, where the board has none. */
  const displayBy = $derived(displayByOffered(view.fields.viewType));
  const placedByName = $derived(displayByName(view, templates, m['PropertyType.CreatedTime']()));
  /** A Gantt chart: the card relation it reads as what comes before what. */
  const dependencies = $derived(dependenciesOffered(view.fields.viewType));
  const dependsOnName = $derived(dependenciesName(view, templates, m.viewheader_group_none()));
  const filterCount = $derived(conditionCount(view.fields.filter));
  const sortCount = $derived(view.fields.sortOptions.length);

  let header = $state<HTMLElement | null>(null);

  // What is open. Kept here, above the surfaces, so that it outlives a
  // surface that is rebuilt when a sheet becomes a popover.
  let propertiesOpen = $state(false);
  let groupOpen = $state(false);
  let displayOpen = $state(false);
  let dependenciesOpen = $derived<boolean>(dependencies && false); // closed when the tool goes
  let filterOpen = $state(false);
  let sortOpen = $state(false);

  /**
   * After a tool without a button of its own closes (in the folded row; with
   * `always`, in either), the focus goes to "…", where it was opened from.
   */
  function closedAside(open: boolean, always = false): void {
    if (open || (row && !always)) return;
    void tick().then(() => header?.querySelector<HTMLElement>('[data-tool="more"]')?.focus());
  }

  // ------------------------------------------------------------ search

  /** The view whose search field was opened in the folded row. Another view starts closed. */
  let searchOpenIn = $state<string | undefined>(undefined);
  /** The folded row shows the search field in place of everything else. */
  const searchBar = $derived(!row && (searchOpenIn === view.id || search !== ''));
  let searchField = $state<HTMLInputElement | null>(null);

  async function openSearch(): Promise<void> {
    searchOpenIn = view.id;
    await tick();
    searchField?.focus();
  }

  async function closeSearch(): Promise<void> {
    search = '';
    searchOpenIn = undefined;
    await tick();
    header?.querySelector<HTMLElement>('[data-tool="search"]')?.focus();
  }

  function searchKeys(event: KeyboardEvent): void {
    if (event.key !== 'Escape' || isComposing(event)) return;
    if (row) search = '';
    else void closeSearch();
  }

  // ------------------------------------------------------------- views

  async function addView(viewType: AddableViewType): Promise<void> {
    const made = await board.createView({
      title: NEW_VIEW_TITLES[viewType](),
      fields: newViewFields(viewType, templates),
    });
    if (made) onOpenView(made.id);
  }

  const onAddView = $derived(
    canEdit ? (viewType: AddableViewType) => void addView(viewType) : undefined,
  );

  // ---------------------------------------------------------- grouping

  function groupBy(template: PropertyTemplate | undefined): void {
    if ((template?.id ?? '') === (view.fields.groupById ?? '')) return;
    // One change, so that one undo brings back the grouping and its columns.
    void board.setViewFields(view.id, groupingFields(template), 'groupBy');
  }

  const groupActions = $derived<MenuAction[]>([
    ...(grouping.optional
      ? [
          {
            id: 'none',
            label: m['GroupBy.ungroup'](),
            checked: groupedBy === undefined,
            run: () => {
              groupBy(undefined);
            },
          },
        ]
      : []),
    ...groupableProperties(templates).map((template, at) => ({
      id: template.id,
      label: template.name,
      checked: template.id === groupedBy?.id,
      separated: grouping.optional && at === 0,
      run: () => {
        groupBy(template);
      },
    })),
  ]);

  // --------------------------------------- display by, and dependencies

  /** The lines of a list of choices: choosing one that changes the view writes it. */
  function choiceActions(
    choices: readonly DisplayByChoice[],
    member: 'dateDisplayPropertyId' | 'dependencyPropertyId',
  ): MenuAction[] {
    return choices.map(({ write, ...line }) => ({
      ...line,
      // The line of "none" has the empty identifier, which names nothing in the markup.
      id: line.id || '(none)',
      run: () => {
        if (write) void board.setViewFields(view.id, { [member]: line.id }, member);
      },
    }));
  }
  const displayActions = $derived(
    choiceActions(displayByChoices(view, templates, placedByName), 'dateDisplayPropertyId'),
  );
  const dependenciesActions = $derived(
    choiceActions(
      dependenciesChoices(view, templates, m.viewheader_group_none()),
      'dependencyPropertyId',
    ),
  );

  // ------------------------------------------------------------- "…"

  /** The lines of "…" for the tools that have no button of their own in the row as it is. */
  const asideActions = $derived(
    asideTools(
      {
        row,
        sort: sorting ? sortCount : undefined,
        displayBy: displayBy ? placedByName : undefined,
        dependencies: dependencies ? dependsOnName : undefined,
        group: grouping.offered ? groupName : undefined,
      },
      {
        sort: () => void (sortOpen = true),
        'display-by': () => void (displayOpen = true),
        dependencies: () => void (dependenciesOpen = true),
        group: () => void (groupOpen = true),
        properties: () => void (propertiesOpen = true),
      },
    ),
  );

  /** A tool that is not drawn: an unseen element where "…" is in the row, for its popover to open at. */
  const ASIDE = 'invisible absolute top-1/2 size-8 -translate-y-1/2 roomy:size-touch';
  const asideAt = $derived([ASIDE, row ? 'right-26' : 'right-1.5']);
</script>

<!--
  The button of a tool. In the whole row: its icon, its word, and a count.
  In the folded row: the icon and the count, or (`aside`) nothing to see;
  `always`: nothing to see in either row.
-->
{#snippet tool(
  props: Record<string, unknown>,
  id: string,
  ToolIcon: Icon,
  label: string,
  count: number,
  aside: boolean | 'always',
)}
  <Button
    {...props}
    variant="ghost"
    data-tool={id}
    aria-label={count > 0 ? `${label} (${count})` : label}
    class={[
      'shrink-0 roomy:h-touch',
      aside === 'always' || (aside && !row)
        ? asideAt
        : row
          ? 'max-w-56'
          : 'gap-1 px-2 roomy:min-w-touch',
    ]}
  >
    <!--
      The whole row has the word and no icon: four icons are the width of a
      view's tab, and a pane of 992 pixels has no tab to spare. To have both,
      draw the icon here too, with `data-icon="inline-start"`.
    -->
    {#if row}
      <span class="truncate">{label}</span>
    {:else}
      <ToolIcon />
    {/if}
    {#if count > 0}
      <span
        class="inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-full bg-primary px-1 text-xs leading-none font-semibold text-primary-foreground"
        data-count
      >
        {count}
      </span>
    {/if}
  </Button>
{/snippet}

<div
  bind:this={header}
  data-testid="view-header"
  data-layout={row ? 'row' : 'folded'}
  class={['relative flex h-12 shrink-0 items-center gap-1 border-b', row ? 'px-4' : 'px-1.5']}
>
  <div class="contents" inert={searchBar}>
    {#if row}
      <ViewTabs
        views={board.orderedViews}
        currentId={view.id}
        {viewHref}
        {onOpenView}
        {onAddView}
      />
    {:else}
      <ViewSwitcher
        views={board.orderedViews}
        current={view}
        boardTitle={board.board?.title}
        {onOpenView}
        {onAddView}
      />
    {/if}
    <div class="min-w-1 flex-1"></div>
    {#if !row}
      <IconButton
        label={m['ViewHeader.search-text']()}
        data-tool="search"
        onclick={() => void openSearch()}
      >
        <SearchIcon />
      </IconButton>
    {/if}

    {#if canEdit}
      <PickerSurface
        bind:open={
          () => propertiesOpen,
          (open: boolean) => {
            propertiesOpen = open;
            closedAside(open);
          }
        }
        presentation={pickers.current}
        title={m['ViewHeader.properties']()}
      >
        {#snippet trigger({ props }: { props: Record<string, unknown> })}
          {@render tool(props, 'properties', EyeIcon, m['ViewHeader.properties'](), 0, true)}
        {/snippet}
        <PropertiesPanel {board} {view} />
      </PickerSurface>

      {#if grouping.offered}
        <ChoiceTool
          bind:open={groupOpen}
          id="group"
          icon={LayersIcon}
          title={m.viewheader_group_by()}
          subtitle={viewTitle(view)}
          word={m['ViewHeader.group-by']({ property: groupName })}
          actions={groupActions}
          {tool}
          onchange={closedAside}
        />
      {/if}

      {#if displayBy}
        <ChoiceTool
          bind:open={displayOpen}
          id="display-by"
          icon={CalendarDaysIcon}
          title={m.viewheader_display_by()}
          subtitle={viewTitle(view)}
          word={m['ViewHeader.display-by']({ property: placedByName })}
          actions={displayActions}
          {tool}
          onchange={closedAside}
        />
      {/if}

      {#if dependencies}
        <ChoiceTool
          bind:open={dependenciesOpen}
          id="dependencies"
          icon={GitForkIcon}
          title={m.viewheader_dependencies()}
          subtitle={viewTitle(view)}
          word={m.viewheader_dependencies()}
          actions={dependenciesActions}
          aside="always"
          {tool}
          onchange={(open: boolean) => {
            closedAside(open, true);
          }}
        />
      {/if}

      <PickerSurface
        bind:open={filterOpen}
        presentation={pickers.current}
        title={m['ViewHeader.filter']()}
      >
        {#snippet trigger({ props }: { props: Record<string, unknown> })}
          {@const word = m['ViewHeader.filter']()}
          {@render tool(props, 'filter', ListFilterIcon, word, filterCount, false)}
        {/snippet}
        <FilterEditor {board} {view} />
      </PickerSurface>

      {#if sorting}
        <PickerSurface
          bind:open={
            () => sortOpen,
            (open: boolean) => {
              sortOpen = open;
              closedAside(open);
            }
          }
          presentation={pickers.current}
          title={m['ViewHeader.sort']()}
        >
          {#snippet trigger({ props }: { props: Record<string, unknown> })}
            {@const word = m['ViewHeader.sort']()}
            {@render tool(props, 'sort', ArrowUpDownIcon, word, sortCount, true)}
          {/snippet}
          <SortEditor {board} {view} />
        </PickerSurface>
      {/if}
    {/if}
  </div>

  <!--
    The search field is one element in both rows, so that it keeps what was
    typed, the caret and the focus when the pane changes its width. In the
    folded row it is either away or laid over the whole row.
  -->
  <div
    role="search"
    data-testid="view-search"
    class={row
      ? 'w-40 shrink-0'
      : searchBar
        ? 'absolute inset-0 z-10 flex items-center gap-1 bg-background px-1.5'
        : 'hidden'}
  >
    {#if !row}
      <IconButton
        label={m.viewheader_search_close()}
        data-tool="search-close"
        onclick={() => void closeSearch()}
      >
        <ArrowLeftIcon />
      </IconButton>
    {/if}
    <div class="relative min-w-0 flex-1">
      <SearchIcon
        class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
        aria-hidden="true"
      />
      <Input
        bind:ref={searchField}
        bind:value={search}
        type="search"
        aria-label={m['ViewHeader.search-text']()}
        placeholder={m['ViewHeader.search-text']()}
        autocomplete="off"
        enterkeyhint="search"
        class="pl-8 roomy:h-touch roomy:text-base"
        onkeydown={searchKeys}
      />
    </div>
  </div>

  {#if canEdit}
    <div class="contents" inert={searchBar}>
      <ViewMenu {board} {view} tools={asideActions} {onOpenView} />
      {#if row}
        <Button class="ml-1 shrink-0 roomy:h-touch" data-testid="new-card" onclick={onAddCard}>
          <PlusIcon data-icon="inline-start" />
          {m['ViewHeader.new']()}
        </Button>
      {/if}
    </div>
  {/if}
</div>

{#if canEdit && !row}
  <!--
    "New" in the folded layout: a round button at the bottom of the board,
    where a thumb is. Placed against the box of the board page; it stays
    clear of the home indicator and rides on top of the on-screen keyboard.
  -->
  <Button
    aria-label={m['ViewHeader.new']()}
    title={m['ViewHeader.new']()}
    data-testid="new-card"
    class="absolute right-4 bottom-[calc(1rem+var(--safe-bottom)+var(--keyboard))] z-20 size-14 rounded-full shadow-lg"
    onclick={onAddCard}
  >
    <PlusIcon class="size-6" />
  </Button>
{/if}
