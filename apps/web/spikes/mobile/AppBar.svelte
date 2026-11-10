<!--
  The top row of the pane: which board this is.

  What stands at its left depends on where the list of boards is (`leading`):
  the button that slides the sidebar in (`menu`), the one that folds a fixed
  sidebar to a strip and back (`toggle`), nothing when a strip has its own
  button (`none`), or, as the second proposal for a narrow window, the name
  of the board as a button that opens the board switcher (`switcher`).

  The rest follows the width of the pane it is in, not of the window: the
  description and the faces of the members appear when there is room for
  them. The top of a phone is the hardest place for a thumb to reach, so a
  narrow pane keeps only what is used once in a while.
-->
<script lang="ts">
  import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
  import MenuIcon from '@lucide/svelte/icons/menu';
  import PanelLeftIcon from '@lucide/svelte/icons/panel-left';
  import UsersIcon from '@lucide/svelte/icons/users';

  import { Button } from '$lib/ui/button/index.js';

  import Avatar from './Avatar.svelte';
  import BoardIcon from './BoardIcon.svelte';
  import { BOARD, BOARD_DESCRIPTION, ME, PEOPLE } from './data.ts';
  import IconButton from './IconButton.svelte';
  import { T } from './text.ts';

  interface Props {
    leading: 'menu' | 'toggle' | 'none' | 'switcher';
    onmenu: () => void;
    onswitch: () => void;
  }

  let { leading, onmenu, onswitch }: Props = $props();
</script>

<header
  data-testid="app-bar"
  class="@columns/pane:h-14 @columns/pane:gap-2 @columns/pane:border-b-0 @columns/pane:px-4 flex h-12 shrink-0 items-center gap-0.5 border-b px-1.5"
>
  {#if leading === 'menu'}
    <IconButton label={T.openSidebar} data-testid="open-sidebar" onclick={onmenu}>
      <MenuIcon />
    </IconButton>
  {:else if leading === 'toggle'}
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label={T.closeSidebar}
      data-testid="toggle-sidebar"
      class="-ml-1.5 pointer-coarse:size-touch"
      onclick={onmenu}
    >
      <PanelLeftIcon />
    </Button>
  {/if}

  {#if leading === 'switcher'}
    <Button
      variant="ghost"
      aria-haspopup="dialog"
      data-testid="open-switcher"
      class="min-w-0 shrink gap-2 pointer-coarse:h-touch"
      onclick={onswitch}
    >
      <BoardIcon board={BOARD} />
      <span class="truncate font-semibold">{BOARD.name}</span>
      <ChevronsUpDownIcon class="text-muted-foreground" />
    </Button>
  {:else}
    <div class="@columns/pane:px-0 flex min-w-0 shrink items-center gap-2 px-1">
      <BoardIcon board={BOARD} />
      <h1 class="@columns/pane:text-lg truncate font-semibold">{BOARD.name}</h1>
    </div>
  {/if}
  <p class="@header/pane:block hidden min-w-0 truncate text-muted-foreground">
    {BOARD_DESCRIPTION}
  </p>
  <div class="flex-1"></div>

  <div class="@columns/pane:flex hidden -space-x-1.5">
    {#each PEOPLE.slice(0, 4) as person (person.id)}
      <Avatar {person} class="size-7 ring-2 ring-background" />
    {/each}
  </div>
  <!-- An icon alone in a narrow pane, a button with its word where there is room. -->
  <IconButton label={T.share} class="@columns/pane:hidden"><UsersIcon /></IconButton>
  <Button variant="outline" class="@columns/pane:inline-flex hidden pointer-coarse:h-touch">
    <UsersIcon data-icon="inline-start" />
    {T.share}
  </Button>
  {#if leading === 'switcher'}
    <!-- Without the drawer, the account and the settings have to be reached from here. -->
    <IconButton label={T.account}><Avatar person={ME} decorative /></IconButton>
  {/if}
</header>
