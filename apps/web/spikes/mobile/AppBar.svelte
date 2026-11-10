<!--
  The top row: which board this is.

  On a narrow screen it is also the way to the other boards, in one of two
  forms (`boards`): a button at the left that slides the sidebar in
  (`drawer`), or the name of the board as a button that opens the board
  switcher (`switcher`). The top of the screen is the hardest place for a
  thumb to reach, so it holds only what is used once in a while.
-->
<script lang="ts">
  import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
  import MenuIcon from '@lucide/svelte/icons/menu';
  import UsersIcon from '@lucide/svelte/icons/users';

  import { Button } from '$lib/ui/button/index.js';
  import * as Sidebar from '$lib/ui/sidebar/index.js';

  import Avatar from './Avatar.svelte';
  import BoardIcon from './BoardIcon.svelte';
  import { BOARD, BOARD_DESCRIPTION, ME, PEOPLE } from './data.ts';
  import IconButton from './IconButton.svelte';
  import { narrow } from './layout.svelte.ts';
  import { T } from './text.ts';

  interface Props {
    boards: 'drawer' | 'switcher';
    onswitch: () => void;
  }

  let { boards, onswitch }: Props = $props();

  const sidebar = Sidebar.useSidebar();
</script>

{#if narrow.current}
  <header data-testid="app-bar" class="flex h-12 shrink-0 items-center gap-0.5 border-b px-1.5">
    {#if boards === 'drawer'}
      <IconButton
        label={T.openSidebar}
        data-testid="open-sidebar"
        onclick={() => {
          sidebar.toggle();
        }}
      >
        <MenuIcon />
      </IconButton>
      <div class="flex min-w-0 flex-1 items-center gap-2 px-1">
        <BoardIcon board={BOARD} />
        <h1 class="truncate font-semibold">{BOARD.name}</h1>
      </div>
    {:else}
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
      <div class="flex-1"></div>
    {/if}
    <IconButton label={T.share}><UsersIcon /></IconButton>
    {#if boards === 'switcher'}
      <!-- Without the drawer, the account and the settings have to be reached from here. -->
      <IconButton label={T.account}><Avatar person={ME} decorative /></IconButton>
    {/if}
  </header>
{:else}
  <header data-testid="app-bar" class="flex h-14 shrink-0 items-center gap-2 px-4">
    <Sidebar.Trigger class="-ml-1.5 pointer-coarse:size-touch" />
    <BoardIcon board={BOARD} />
    <h1 class="shrink-0 text-lg font-semibold">{BOARD.name}</h1>
    <p class="hidden min-w-0 truncate text-muted-foreground lg:block">{BOARD_DESCRIPTION}</p>
    <div class="flex-1"></div>
    <div class="flex -space-x-1.5">
      {#each PEOPLE.slice(0, 4) as person (person.id)}
        <Avatar {person} class="size-7 ring-2 ring-background" />
      {/each}
    </div>
    <Button variant="outline" class="pointer-coarse:h-touch">
      <UsersIcon data-icon="inline-start" />
      {T.share}
    </Button>
  </header>
{/if}
