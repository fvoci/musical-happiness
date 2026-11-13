<!--
  The top row of the pane: which board this is, who is on it, sharing it
  (for its admins), and undo and redo, which pulse (or fade, with less
  motion) while one asked for waits for a write.

  What it shows follows the width of the pane it is in, not of the window:
  the faces of the members from where two kanban columns fit, the
  description of the board from where the whole header of a view fits. A
  narrow pane keeps the title and the two arrows.

  Its editors find the board's deleted cards here ("Deleted cards").

  The title is a field for a reader who may change the board, and plain text
  for one who may not; a board that can only be read says so next to it. The
  button that slides the sidebar in stands at the left where nothing else on
  the page opens it (a compact page).
-->
<script lang="ts">
  import MenuIcon from '@lucide/svelte/icons/menu';
  import Redo2Icon from '@lucide/svelte/icons/redo-2';
  import Share2Icon from '@lucide/svelte/icons/share-2';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import Undo2Icon from '@lucide/svelte/icons/undo-2';

  import PresenceRoster from './PresenceRoster.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { cardTrash, type Trash, type Workspace } from '$lib/state/index.ts';

  import { Button } from '$lib/ui/button/index.js';

  import BoardIcon from './BoardIcon.svelte';
  import BoardTitle from './BoardTitle.svelte';
  import ShareDialog from './ShareDialog.svelte';
  import TrashDialog from './TrashDialog.svelte';

  interface Props {
    workspace: Workspace;
    /** The board on the page. */
    boardId: string | undefined;
    /** The address of a board's public link with its read token. */
    sharedHref: (boardId: string, readToken: string) => string;
    /** Show the button that opens the drawer. */
    menuButton: boolean;
    onOpenSidebar: () => void;
  }

  let { workspace, boardId, sharedHref, menuButton, onOpenSidebar }: Props = $props();

  let sharing = $state(false);
  let trashOpen = $state(false);
  /** The deleted cards of the board, read anew each time they are shown. */
  let trash = $state.raw<Trash | undefined>(undefined);

  /** The board as the list of boards has it: known before the board itself is read. */
  const listed = $derived(workspace.boards.find((candidate) => candidate.id === boardId));
  /** The open board, once it is read: who is on it and what the reader may do. */
  const store = $derived(
    workspace.board?.boardId === boardId && workspace.board?.status === 'ready'
      ? workspace.board
      : undefined,
  );
  const board = $derived(store?.board ?? listed);
  /** Sharing is for the board's admins; a template is shared as it is made, never joined. */
  const canShare = $derived(store?.canShare === true && store.board?.isTemplate !== true);
  /** The trash of cards is for the board's editors, as the server has it. */
  const canRestoreCards = $derived(store?.canEdit === true);

  function openTrash(): void {
    if (!store) return;
    trash = cardTrash(store);
    trashOpen = true;
    void trash.read();
  }
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
            <!-- The name of a board without one is the same wherever a name is shown; the field's placeholder is the other message. -->
            {board.title || m['Sidebar.untitled-board']()}
          </span>
        {/if}
      </h1>
    </div>
    {#if board.isTemplate}
      <span
        data-testid="template-note"
        title={m['Workspace.editing-board-template']()}
        class="shrink-0 rounded-md border border-dashed px-1.5 py-0.5 text-xs text-muted-foreground"
      >
        <span aria-hidden="true">{m.shell_template_note()}</span>
        <span class="sr-only">{m['Workspace.editing-board-template']()}</span>
      </span>
    {/if}
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

  {#if store && !store.missing && !store.joinable}
    <PresenceRoster board={store} />
  {/if}

  {#if canShare}
    <Button
      variant="outline"
      size="sm"
      data-testid="share-board"
      aria-haspopup="dialog"
      class="shrink-0 roomy:min-h-touch"
      onclick={() => {
        sharing = true;
      }}
    >
      <Share2Icon aria-hidden="true" />
      <span class="sr-only @header/pane:not-sr-only">{m.shell_share()}</span>
    </Button>
  {/if}

  {#if canRestoreCards}
    <IconButton
      label={m.shell_trash_cards()}
      data-testid="open-card-trash"
      aria-haspopup="dialog"
      onclick={openTrash}
    >
      <Trash2Icon />
    </IconButton>
  {/if}

  <!-- An undo or redo waiting for a write says so on both, until it is carried out. -->
  <IconButton
    label={m['UndoRedoHotKeys.canUndo']()}
    data-testid="undo"
    disabled={!workspace.canUndo}
    aria-busy={workspace.undoBusy}
    class="aria-busy:motion-safe:animate-pulse aria-busy:motion-reduce:opacity-60"
    onclick={() => void workspace.undoLast()}
  >
    <Undo2Icon />
  </IconButton>
  <IconButton
    label={m['UndoRedoHotKeys.canRedo']()}
    data-testid="redo"
    disabled={!workspace.canRedo}
    aria-busy={workspace.undoBusy}
    class="aria-busy:motion-safe:animate-pulse aria-busy:motion-reduce:opacity-60"
    onclick={() => void workspace.redoLast()}
  >
    <Redo2Icon />
  </IconButton>
</header>

{#if store && canRestoreCards}
  <TrashDialog
    bind:open={trashOpen}
    {trash}
    {workspace}
    title={m.shell_trash_cards()}
    description={m.shell_trash_cards_description()}
    untitled={m['KanbanCard.untitled']()}
    testId="card-trash"
  />
{/if}

{#if store?.board && canShare}
  <ShareDialog
    bind:open={sharing}
    {workspace}
    board={store}
    linkOf={(readToken: string) =>
      new URL(sharedHref(store.boardId, readToken), location.href).href}
  />
{/if}
