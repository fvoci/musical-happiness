<!--
  The categories of the sidebar and the boards under them, in the reader's
  order (`workspace.sidebar`), and the dragging of boards.

  A board is dragged within its category and into another one: with a mouse,
  with a finger that rests on it, or with the keyboard (Space picks it up).
  An open category is a list with places; a folded one is a target without
  places, and a board dropped on its heading is filed last. Nothing moves in
  the data until the drop: while a board is on its way it stays in the
  document out of sight, and a gap shows where it would land.

  While a search narrows the list nothing can be dragged: the places on
  screen are not the places in the category.

  The order of the categories is changed from their menus ("Move up", "Move
  down"), not by dragging.
-->
<script lang="ts">
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import EyeIcon from '@lucide/svelte/icons/eye';
  import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import { readerPointer } from '$lib/components/drag/reader-pointer.ts';
  import { Sortable } from '$lib/drag/sortable.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Board } from '$lib/logic/wire.ts';
  import type { Category, SidebarCategory, Workspace } from '$lib/state/index.ts';

  import { shownTitle } from './shown-title.ts';
  import BoardIcon from './BoardIcon.svelte';
  import { boardDragMessages } from './drag-messages.ts';
  import {
    boardActions,
    categoryActions,
    categoryLabel,
    type SidebarMenus,
  } from './sidebar-actions.ts';
  import { boardMoveOf, dragOrder, dragZones, filterSidebar } from './sidebar-logic.ts';
  import SidebarBoard from './SidebarBoard.svelte';

  interface Props {
    workspace: Workspace;
    /** The board on the page. */
    boardId: string | undefined;
    boardHref: (board: Board) => string;
    /** What the reader typed into the search of the sidebar. */
    query: string;
    /** What the backend found for the query; those the list shows already are left out. */
    found: readonly Board[];
    /** The backend has not answered the query yet. */
    searching: boolean;
    /** Whether the reader may delete the board on the page. Of the others the sidebar does not know. */
    canDeleteOpenBoard: boolean;
    onOpenBoard: (board: Board) => void;
    onNewCategory: () => void;
    onRenameCategory: (category: Category) => void;
    onDeleteCategory: (category: Category) => void;
    onDeleteBoard: (board: Board) => void;
    /** Makes a template of the board, to be changed on its page. */
    onTemplateFromBoard: (boardId: string) => void;
  }

  let {
    workspace,
    boardId,
    boardHref,
    query,
    found,
    searching,
    canDeleteOpenBoard,
    onOpenBoard,
    onNewCategory,
    onRenameCategory,
    onDeleteCategory,
    onDeleteBoard,
    onTemplateFromBoard,
  }: Props = $props();

  const uid = $props.id();
  const instructionsId = `${uid}-drag`;

  const filtering = $derived(query.trim() !== '');
  const shown = $derived(filterSidebar(workspace.sidebar, query));
  /**
   * Found by the backend and not in the list as it is narrowed: hidden, the words in another
   * order, or of another workspace (the server searches them all), linked at its own. One of
   * this workspace is as the workspace has it: deleted or renamed since the server answered.
   */
  const more = $derived(
    filtering
      ? found.flatMap((board) => {
          if (board.workspaceId !== workspace.info?.id) return [board];
          const known = workspace.boards.find((one) => one.id === board.id);
          const listed = shown.some((entry) => entry.boards.some((one) => one.id === board.id));
          return known && !listed ? [known] : [];
        })
      : [],
  );
  const isHidden = (boardId: string): boolean =>
    workspace.categories.some((category) => category.hiddenBoardIds.includes(boardId));

  const titleOf = shownTitle;
  const boardOf = (id: string): Board | undefined =>
    workspace.boards.find((board) => board.id === id);
  const categoryName = (id: string): string => {
    const category = workspace.categories.find((one) => one.id === id);
    return category ? categoryLabel(category) : id;
  };

  /** A guest has the boards they were invited to as they are filed: the sidebar is not theirs to arrange. */
  const guest = $derived(workspace.info?.role === 'guest');
  /** What the menus of the categories and the boards act with (`sidebar-actions.ts`). */
  const menus = $derived<SidebarMenus>({
    workspace,
    guest,
    boardId,
    canDeleteOpenBoard,
    onNewCategory,
    onRenameCategory,
    onDeleteCategory,
    onDeleteBoard,
    onTemplateFromBoard,
  });

  const sortable = new Sortable({
    order: () => (filtering || guest ? [] : dragOrder(workspace.sidebar)),
    zones: () => (filtering || guest ? [] : dragZones(workspace.sidebar)),
    onDrop: ({ itemId, to }) => {
      const move = boardMoveOf(to, itemId, workspace.sidebar);
      if (move) void workspace.moveBoard(move.boardId, move.categoryId, move.index);
    },
    messages: boardDragMessages(),
    itemLabel: (id) => {
      const board = boardOf(id);
      return board ? titleOf(board) : id;
    },
    listLabel: categoryName,
    zoneLabel: categoryName,
    // The reader's setting decides whether a lifted board buzzes.
    pointer: readerPointer,
  });

  /** Whether the list of hidden boards is open. */
  let hiddenOpen = $state(false);

  const drag = $derived(sortable.active);
  const lifted = $derived(drag ? boardOf(drag.itemId) : undefined);

  type Row = { kind: 'board'; board: Board } | { kind: 'gap' };

  /** The boards of a category with the gap among them, where a drag has one there. */
  function rowsOf(entry: SidebarCategory): Row[] {
    const gapAt = drag?.gap?.listId === entry.category.id ? drag.gap.index : -1;
    const rows: Row[] = [];
    let staying = 0;
    for (const board of entry.boards) {
      if (!drag?.itemIds.includes(board.id)) {
        if (staying === gapAt) rows.push({ kind: 'gap' });
        staying += 1;
      }
      rows.push({ kind: 'board', board });
    }
    if (staying === gapAt) rows.push({ kind: 'gap' });
    return rows;
  }
</script>

<div
  class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-2"
  data-testid="sidebar-list"
  data-drag-scroll="y"
  data-dragging={drag?.mode}
  {@attach sortable.attach}
>
  <p id={instructionsId} class="sr-only">{m.shell_drag_instructions()}</p>

  {#each shown as entry (entry.category.id)}
    {@const category = entry.category}
    {@const open = filtering || !category.collapsed}
    {@const over = drag?.to?.kind === 'zone' && drag.to.zoneId === category.id}
    <div class="pt-2" data-category={category.id}>
      <div
        class={['group/category relative rounded-md', over && 'bg-sidebar-accent ring-2 ring-ring']}
        data-drag-zone={open ? undefined : category.id}
      >
        <button
          type="button"
          data-category-toggle
          aria-expanded={open}
          disabled={filtering || guest}
          class="flex min-h-8 w-full items-center gap-1 rounded-md py-1 pr-9 pl-1 text-left text-xs font-medium text-muted-foreground hover:bg-sidebar-accent disabled:hover:bg-transparent roomy:min-h-touch roomy:pr-12"
          onclick={() => void workspace.setCategoryCollapsed(category.id, !category.collapsed)}
        >
          <ChevronRightIcon
            class={['size-4 shrink-0 transition-transform', open && 'rotate-90']}
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1 truncate">{categoryLabel(category)}</span>
        </button>
        {#if !guest}
          <ActionMenu title={categoryLabel(category)} actions={categoryActions(menus, category)}>
            {#snippet trigger(props: Record<string, unknown>)}
              <IconButton
                {...props}
                label={m.shell_category_menu({ name: categoryLabel(category) })}
                class="absolute top-0 right-0 text-muted-foreground can-hover:opacity-0 can-hover:group-focus-within/category:opacity-100 can-hover:group-hover/category:opacity-100 can-hover:aria-expanded:opacity-100"
              >
                <EllipsisIcon />
              </IconButton>
            {/snippet}
          </ActionMenu>
        {/if}
      </div>

      {#if open}
        {@const rows = rowsOf(entry)}
        <ul
          class="min-h-8 roomy:min-h-touch"
          data-drag-list={filtering ? undefined : category.id}
          aria-label={categoryLabel(category)}
        >
          {#each rows as row (row.kind === 'gap' ? ' gap' : row.board.id)}
            {#if row.kind === 'board'}
              <SidebarBoard
                boardId={row.board.id}
                title={titleOf(row.board)}
                icon={row.board.icon}
                kind={row.board.kind}
                href={boardHref(row.board)}
                current={row.board.id === boardId}
                locale={workspace.locale}
                draggable={!filtering}
                lifted={drag?.itemIds.includes(row.board.id) ?? false}
                describedBy={instructionsId}
                actions={boardActions(menus, row.board, category)}
                onOpen={() => {
                  onOpenBoard(row.board);
                }}
              />
            {:else if drag?.mode === 'keyboard' && lifted}
              <li>
                <div
                  data-drag-carried={lifted.id}
                  role="button"
                  tabindex="0"
                  aria-describedby={instructionsId}
                  class="flex min-h-8 items-center gap-2 rounded-md bg-sidebar-accent py-1 pr-2 pl-2 font-medium ring-2 ring-ring outline-none roomy:min-h-touch"
                >
                  <BoardIcon
                    icon={lifted.icon}
                    kind={lifted.kind}
                    title={titleOf(lifted)}
                    locale={workspace.locale}
                  />
                  <span class="min-w-0 flex-1 truncate">{titleOf(lifted)}</span>
                </div>
              </li>
            {:else}
              <li
                data-drag-gap
                aria-hidden="true"
                class="rounded-md border-2 border-dashed border-input-border"
                style:height="{drag?.height ?? 0}px"
              ></li>
            {/if}
          {/each}
          {#if rows.length === 0}
            <li class="px-2 py-1.5 text-xs text-muted-foreground roomy:py-3">
              {m['Sidebar.no-boards-in-category']()}
            </li>
          {/if}
        </ul>
      {/if}
    </div>
  {/each}

  {#if more.length > 0}
    <div class="pt-2" data-testid="search-more">
      <p class="px-1 py-1 text-xs font-medium text-muted-foreground">{m.shell_search_more()}</p>
      <ul aria-label={m.shell_search_more()}>
        {#each more as board (board.id)}
          {@const hidden = isHidden(board.id)}
          <li class="flex items-center gap-1">
            <a
              href={boardHref(board)}
              aria-current={board.id === boardId ? 'page' : undefined}
              class="flex min-h-8 min-w-0 flex-1 items-center gap-2 rounded-md py-1 pl-2 hover:bg-sidebar-accent roomy:min-h-touch"
              onclick={(event: MouseEvent) => {
                if (
                  event.button !== 0 ||
                  event.metaKey ||
                  event.ctrlKey ||
                  event.shiftKey ||
                  event.altKey
                ) {
                  return;
                }
                event.preventDefault();
                onOpenBoard(board);
              }}
            >
              <BoardIcon
                icon={board.icon}
                kind={board.kind}
                title={titleOf(board)}
                locale={workspace.locale}
              />
              <span class="min-w-0 flex-1 truncate">{titleOf(board)}</span>
              {#if hidden}
                <span class="shrink-0 pr-1 text-xs text-muted-foreground">
                  {m.shell_search_hidden()}
                </span>
              {/if}
            </a>
            {#if hidden}
              <IconButton
                label={m.shell_board_unhide({ name: titleOf(board) })}
                class="text-muted-foreground"
                onclick={() => void workspace.unhideBoard(board.id)}
              >
                <EyeIcon />
              </IconButton>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if filtering && shown.length === 0 && more.length === 0 && !searching}
    <p class="px-2 py-3 text-muted-foreground" role="status">{m.shell_no_boards_found()}</p>
  {/if}

  {#if !filtering && workspace.hiddenBoards.length > 0}
    <div class="pt-2" data-testid="hidden-boards">
      <button
        type="button"
        aria-expanded={hiddenOpen}
        class="flex min-h-8 w-full items-center gap-1 rounded-md py-1 pl-1 text-left text-xs font-medium text-muted-foreground hover:bg-sidebar-accent roomy:min-h-touch"
        onclick={() => {
          hiddenOpen = !hiddenOpen;
        }}
      >
        <ChevronRightIcon
          class={['size-4 shrink-0 transition-transform', hiddenOpen && 'rotate-90']}
          aria-hidden="true"
        />
        <span class="min-w-0 flex-1 truncate">
          {m.shell_hidden_boards({ count: workspace.hiddenBoards.length })}
        </span>
      </button>
      {#if hiddenOpen}
        <ul>
          {#each workspace.hiddenBoards as board (board.id)}
            <li class="flex items-center gap-1">
              <a
                href={boardHref(board)}
                aria-current={board.id === boardId ? 'page' : undefined}
                class="flex min-h-8 min-w-0 flex-1 items-center gap-2 rounded-md py-1 pl-2 text-muted-foreground hover:bg-sidebar-accent roomy:min-h-touch"
                onclick={(event: MouseEvent) => {
                  if (
                    event.button !== 0 ||
                    event.metaKey ||
                    event.ctrlKey ||
                    event.shiftKey ||
                    event.altKey
                  ) {
                    return;
                  }
                  event.preventDefault();
                  onOpenBoard(board);
                }}
              >
                <BoardIcon
                  icon={board.icon}
                  kind={board.kind}
                  title={titleOf(board)}
                  locale={workspace.locale}
                />
                <span class="min-w-0 flex-1 truncate">{titleOf(board)}</span>
              </a>
              <IconButton
                label={m.shell_board_unhide({ name: titleOf(board) })}
                class="text-muted-foreground"
                onclick={() => void workspace.unhideBoard(board.id)}
              >
                <EyeIcon />
              </IconButton>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}

  {#if !filtering && !guest}
    <button
      type="button"
      data-testid="new-category"
      class="mt-2 flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-muted-foreground hover:bg-sidebar-accent hover:text-foreground roomy:min-h-touch"
      onclick={onNewCategory}
    >
      <FolderPlusIcon class="size-4 shrink-0" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{m['SidebarCategories.CategoryMenu.CreateNew']()}</span>
    </button>
  {/if}

  {#if sortable.notice}
    <p class="sticky bottom-0 mt-2 rounded-md bg-popover px-2 py-1.5 text-xs shadow-md">
      {sortable.notice}
    </p>
  {/if}
  <div class="sr-only" aria-live="assertive" aria-atomic="true" data-testid="sidebar-announcer">
    {sortable.announcement}
  </div>
</div>

<!-- The board a pointer drags: it follows the pointer, over everything, and takes no presses. -->
{#if drag?.mode === 'pointer' && sortable.overlay && lifted}
  <div
    data-drag-overlay={lifted.id}
    aria-hidden="true"
    class="pointer-events-none fixed top-0 left-0 z-[70] flex items-center gap-2 rounded-md bg-sidebar-accent py-1 pr-2 pl-2 font-medium text-sidebar-accent-foreground shadow-lg ring-2 ring-ring will-change-transform"
    style:width="{drag.width}px"
    style:height="{drag.height}px"
    style:translate="{sortable.overlay.x}px {sortable.overlay.y}px"
  >
    <BoardIcon
      icon={lifted.icon}
      kind={lifted.kind}
      title={titleOf(lifted)}
      locale={workspace.locale}
    />
    <span class="min-w-0 flex-1 truncate">{titleOf(lifted)}</span>
  </div>
{/if}
