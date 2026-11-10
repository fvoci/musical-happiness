<!--
  The categories and their boards. The generated sidebar decides the form by
  the width of the screen: fixed at the left on a wide one, a drawer that
  slides in from the left on a narrow one (a sheet: it dims the rest, and a
  tap outside, a board, or the back button closes it).

  Where a finger is used a line is `--spacing-touch` high instead of 2rem.
-->
<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SettingsIcon from '@lucide/svelte/icons/settings';

  import * as Sidebar from '$lib/ui/sidebar/index.js';

  import Avatar from './Avatar.svelte';
  import BoardIcon from './BoardIcon.svelte';
  import { BOARD, CATEGORIES, ME } from './data.ts';
  import { closeOnBack } from './history.svelte.ts';
  import IconButton from './IconButton.svelte';
  import { T } from './text.ts';

  interface Props {
    /** The drawer is out when the page loads. */
    openOnLoad: boolean;
    onfind: () => void;
  }

  let { openOnLoad, onfind }: Props = $props();

  const sidebar = Sidebar.useSidebar();

  $effect.pre(() => {
    if (openOnLoad) sidebar.setOpenMobile(true);
  });

  closeOnBack(
    () => sidebar.isMobile && sidebar.openMobile,
    () => {
      sidebar.setOpenMobile(false);
    },
  );
</script>

<Sidebar.Root data-testid="sidebar">
  <Sidebar.Header class="pt-[calc(0.5rem+var(--safe-top))]">
    <div class="flex h-8 items-center justify-between pl-2 pointer-coarse:h-touch">
      <span class="text-lg font-semibold tracking-tight">{T.logo}</span>
      <IconButton label={T.settings}><SettingsIcon /></IconButton>
    </div>
    <button
      type="button"
      class="flex h-8 items-center gap-2 rounded-lg border border-input-border bg-background px-2.5 text-left text-muted-foreground pointer-coarse:h-touch"
      onclick={() => {
        if (sidebar.isMobile) sidebar.setOpenMobile(false);
        onfind();
      }}
    >
      <SearchIcon class="size-4 shrink-0" />
      <span class="flex-1 truncate">{T.findBoard}</span>
      <kbd class="hidden font-sans text-xs md:inline">Ctrl K</kbd>
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
                    if (sidebar.isMobile) sidebar.setOpenMobile(false);
                  }}
                >
                  <BoardIcon {board} />
                  <span>{board.name}</span>
                </Sidebar.MenuButton>
                <Sidebar.MenuAction
                  showOnHover
                  aria-label={T.boardMenu(board.name)}
                  class="pointer-coarse:top-3!"
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
</Sidebar.Root>
