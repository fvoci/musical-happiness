<!--
  The top row of the pane: which board this is, who is on it, and undo and
  redo.

  What it shows follows the width of the pane it is in, not of the window:
  the faces of the members from where two kanban columns fit, the
  description of the board from where the whole header of a view fits. A
  narrow pane keeps the title and the two arrows.

  The title is a field for a reader who may change the board, and plain text
  for one who may not; a board that can only be read says so next to it. The
  button that slides the sidebar in stands at the left where nothing else on
  the page opens it (a compact page).
-->
<script lang="ts">
  import MenuIcon from '@lucide/svelte/icons/menu';
  import Redo2Icon from '@lucide/svelte/icons/redo-2';
  import Undo2Icon from '@lucide/svelte/icons/undo-2';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Workspace } from '$lib/state/index.ts';

  import BoardIcon from './BoardIcon.svelte';
  import BoardTitle from './BoardTitle.svelte';

  interface Props {
    workspace: Workspace;
    /** The board on the page. */
    boardId: string | undefined;
    /** Show the button that opens the drawer. */
    menuButton: boolean;
    onOpenSidebar: () => void;
  }

  let { workspace, boardId, menuButton, onOpenSidebar }: Props = $props();

  /** How many faces are shown before the rest become a number. */
  const FACES = 4;

  /** The board as the list of boards has it: known before the board itself is read. */
  const listed = $derived(workspace.boards.find((candidate) => candidate.id === boardId));
  /** The open board, once it is read: who is on it and what the reader may do. */
  const store = $derived(
    workspace.board?.boardId === boardId && workspace.board?.status === 'ready'
      ? workspace.board
      : undefined,
  );
  const board = $derived(store?.board ?? listed);
  const members = $derived(store?.orderedMembers ?? []);
</script>

<header
  data-testid="top-bar"
  class="flex h-12 shrink-0 items-center gap-1 border-b px-1.5 @columns/pane:h-14 @columns/pane:gap-2 @columns/pane:px-3"
>
  {#if menuButton}
    <IconButton label={m.shell_open_sidebar()} data-testid="open-sidebar" onclick={onOpenSidebar}>
      <MenuIcon />
    </IconButton>
  {/if}

  {#if board}
    <div class="flex min-w-0 shrink items-center gap-1.5 pl-1">
      <BoardIcon icon={board.icon} title={board.title} locale={workspace.locale} />
      <h1 class="flex min-w-0 shrink text-base font-semibold @columns/pane:text-lg">
        {#if store?.canEdit}
          {#key board.id}
            <BoardTitle
              title={board.title}
              onChange={(title: string) =>
                void store.changeBoard((current) => ({ ...current, title }), 'changeBoardTitle')}
            />
          {/key}
        {:else}
          <span data-testid="board-title-text" class="min-w-0 truncate px-1.5">
            {board.title || m['ViewTitle.untitled-board']()}
          </span>
        {/if}
      </h1>
    </div>
    {#if store && !store.canEdit}
      <span
        data-testid="read-only"
        class="shrink-0 rounded-md border px-1.5 py-0.5 text-xs text-muted-foreground"
      >
        {m.shared_read_only()}
      </span>
    {/if}
    {#if board.description}
      <p class="hidden min-w-0 shrink-[2] truncate text-muted-foreground @header/pane:block">
        {board.description}
      </p>
    {/if}
  {/if}

  <div class="min-w-0 flex-1"></div>

  {#if members.length > 0}
    <ul
      data-testid="board-members"
      aria-label={m.shell_members()}
      class="hidden shrink-0 items-center @columns/pane:flex"
    >
      {#each members.slice(0, FACES) as member (member.userId)}
        <li class="-ml-1.5 flex first:ml-0">
          <Avatar
            name={workspace.userName(member.userId) ?? m['centerPanel.unknown-user']()}
            colorKey={member.userId}
            locale={workspace.locale}
            class="size-7 ring-2 ring-background"
          />
        </li>
      {/each}
      {#if members.length > FACES}
        <li class="pl-1.5 text-xs text-muted-foreground">
          <span aria-hidden="true">+{members.length - FACES}</span>
          <span class="sr-only">{m.shell_members_more({ count: members.length - FACES })}</span>
        </li>
      {/if}
    </ul>
  {/if}

  <IconButton
    label={m['UndoRedoHotKeys.canUndo']()}
    data-testid="undo"
    disabled={!workspace.canUndo}
    onclick={() => void workspace.undoLast()}
  >
    <Undo2Icon />
  </IconButton>
  <IconButton
    label={m['UndoRedoHotKeys.canRedo']()}
    data-testid="redo"
    disabled={!workspace.canRedo}
    onclick={() => void workspace.redoLast()}
  >
    <Redo2Icon />
  </IconButton>
</header>
