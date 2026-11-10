<!--
  The categories and their boards, in the form the shell asks for:

  - `fixed`: a column at the left, 16rem wide, that the board sits beside.
  - `strip`: a column 3.5rem wide with one button a board. It keeps the way
    to another board one tap away and shows which board this is.
  - `none`: nothing in the row.

  With `overlay`, the whole list can also slide in from the left over the
  page (a sheet: it dims the rest, and a tap outside, a board, or the back
  button closes it). That is the drawer of a narrow window, and what the
  strip of a medium one opens into.

  The frame is drawn here and not by the generated sidebar: that one changes
  from drawer to column at a width of the viewport written into its code
  (768 pixels), and knows no third form. Its inner parts are used as they are.

  Where a finger is used a line is `--spacing-touch` high instead of 2rem.
-->
<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import PanelLeftOpenIcon from '@lucide/svelte/icons/panel-left-open';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SettingsIcon from '@lucide/svelte/icons/settings';

  import * as Sheet from '$lib/ui/sheet/index.js';
  import * as Sidebar from '$lib/ui/sidebar/index.js';

  import Avatar from './Avatar.svelte';
  import BoardIcon from './BoardIcon.svelte';
  import { BOARD, CATEGORIES, ME } from './data.ts';
  import { closeOnBack } from './history.svelte.ts';
  import IconButton from './IconButton.svelte';
  import { T } from './text.ts';

  interface Props {
    form: 'fixed' | 'strip' | 'none';
    overlay: boolean;
    /** The drawer is out. */
    drawerOpen: boolean;
    /** The strip was asked to become the whole list. */
    onexpand: () => void;
    onfind: () => void;
  }

  let { form, overlay, drawerOpen = $bindable(), onexpand, onfind }: Props = $props();

  let drawer = $state<HTMLElement | null>(null);

  closeOnBack(
    () => overlay && drawerOpen,
    () => {
      drawerOpen = false;
    },
  );

  function find(): void {
    drawerOpen = false;
    onfind();
  }

  // Shown once the line is pointed at; always, on a device that cannot point without pressing.
  const ON_HOVER =
    'peer-data-active/menu-button:text-sidebar-accent-foreground can-hover:opacity-0 can-hover:group-focus-within/menu-item:opacity-100 can-hover:group-hover/menu-item:opacity-100 can-hover:aria-expanded:opacity-100';
</script>

{#snippet list(inDrawer: boolean)}
  <Sidebar.Header class={[inDrawer && 'pt-[calc(0.5rem+var(--safe-top))]']}>
    <div class="flex h-8 items-center justify-between pl-2 pointer-coarse:h-touch">
      <span class="text-lg font-semibold tracking-tight">{T.logo}</span>
      <IconButton label={T.settings}><SettingsIcon /></IconButton>
    </div>
    <button
      type="button"
      class="flex h-8 items-center gap-2 rounded-lg border border-input-border bg-background px-2.5 text-left text-muted-foreground pointer-coarse:h-touch"
      onclick={find}
    >
      <SearchIcon class="size-4 shrink-0" />
      <span class="flex-1 truncate">{T.findBoard}</span>
      <!-- A shortcut is for a keyboard; a device whose pointer is a finger seldom has one out. -->
      <kbd class="font-sans text-xs pointer-coarse:hidden">Ctrl K</kbd>
    </button>
  </Sidebar.Header>

  <Sidebar.Content>
    {#each CATEGORIES as category (category.id)}
      <Sidebar.Group class="py-1">
        <Sidebar.GroupLabel class="pointer-coarse:h-touch">
          <ChevronDownIcon class="mr-1" />
          {category.name}
        </Sidebar.GroupLabel>
        <Sidebar.GroupAction
          aria-label={T.categoryMenu(category.name)}
          class="top-2.5 pointer-coarse:top-4"
        >
          <EllipsisIcon />
        </Sidebar.GroupAction>
        <Sidebar.GroupContent>
          <Sidebar.Menu>
            {#each category.boards as board (board.id)}
              <Sidebar.MenuItem>
                <Sidebar.MenuButton
                  isActive={board.id === BOARD.id}
                  class="pointer-coarse:h-touch"
                  onclick={() => {
                    drawerOpen = false;
                  }}
                >
                  <BoardIcon {board} />
                  <span>{board.name}</span>
                </Sidebar.MenuButton>
                <Sidebar.MenuAction
                  aria-label={T.boardMenu(board.name)}
                  class={['pointer-coarse:top-3!', ON_HOVER]}
                >
                  <EllipsisIcon />
                </Sidebar.MenuAction>
              </Sidebar.MenuItem>
            {/each}
          </Sidebar.Menu>
        </Sidebar.GroupContent>
      </Sidebar.Group>
    {/each}
  </Sidebar.Content>

  <Sidebar.Footer class="border-t pb-[calc(0.5rem+var(--safe-bottom))]">
    <Sidebar.Menu>
      <Sidebar.MenuItem>
        <Sidebar.MenuButton class="pointer-coarse:h-touch">
          <PlusIcon />
          <span>{T.addBoard}</span>
        </Sidebar.MenuButton>
      </Sidebar.MenuItem>
      <Sidebar.MenuItem>
        <Sidebar.MenuButton class="pointer-coarse:h-touch">
          <Avatar person={ME} decorative class="size-5" />
          <span>{ME.name}</span>
        </Sidebar.MenuButton>
      </Sidebar.MenuItem>
    </Sidebar.Menu>
  </Sidebar.Footer>
{/snippet}

{#if form === 'fixed'}
  <!-- The line at its edge is drawn on the page, not on the fill of the sidebar. -->
  <aside data-testid="sidebar" aria-label={T.sidebar} class="flex w-64 shrink-0 border-r">
    <div class="flex min-w-0 flex-1 flex-col bg-sidebar text-sidebar-foreground">
      {@render list(false)}
    </div>
  </aside>
{:else if form === 'strip'}
  <nav
    data-testid="sidebar-strip"
    aria-label={T.boards}
    class="flex w-14 shrink-0 flex-col items-center gap-1 border-r bg-sidebar py-2 text-sidebar-foreground"
  >
    <IconButton label={T.openSidebar} data-testid="expand-sidebar" onclick={onexpand}>
      <PanelLeftOpenIcon />
    </IconButton>
    <IconButton label={T.findBoard} onclick={find}><SearchIcon /></IconButton>
    <div class="my-1 w-8 border-t"></div>
    <div class="no-scrollbar flex min-h-0 flex-1 flex-col items-center gap-1 overflow-y-auto">
      {#each CATEGORIES as category (category.id)}
        {#each category.boards as board (board.id)}
          <IconButton
            label={board.name}
            aria-current={board.id === BOARD.id ? 'page' : undefined}
            class="shrink-0 aria-[current]:bg-sidebar-accent"
          >
            <BoardIcon {board} />
          </IconButton>
        {/each}
      {/each}
    </div>
    <div class="my-1 w-8 border-t"></div>
    <IconButton label={T.addBoard}><PlusIcon /></IconButton>
    <IconButton label={T.account} class="mb-(--safe-bottom)">
      <Avatar person={ME} decorative />
    </IconButton>
  </nav>
{/if}

{#if overlay}
  <Sheet.Root bind:open={drawerOpen}>
    <Sheet.Content
      bind:ref={drawer}
      side="left"
      showCloseButton={false}
      data-testid="sidebar-drawer"
      class="gap-0 bg-sidebar p-0 pl-(--safe-left) text-sidebar-foreground outline-none data-[side=left]:max-w-[calc(18rem+var(--safe-left))] data-[side=left]:sm:max-w-[calc(18rem+var(--safe-left))]"
      onOpenAutoFocus={(event: Event) => {
        event.preventDefault();
        drawer?.focus();
      }}
    >
      <Sheet.Header class="sr-only">
        <Sheet.Title>{T.sidebar}</Sheet.Title>
      </Sheet.Header>
      {@render list(true)}
    </Sheet.Content>
  </Sheet.Root>
{/if}
