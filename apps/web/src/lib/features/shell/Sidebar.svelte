<!--
  The sidebar: the name of the app (or, for who works in several workspaces, the
  one shown, to switch to another: `WorkspaceSwitcher.svelte`), a field that narrows the boards down by
  title (and asks the backend for the boards it finds beyond them), the categories with their boards, and at the foot "Add board",
  the trash of boards (`TrashDialog.svelte`), the way to the organization's screen for
  its administrators, and the reader's account (which opens the settings).

  This is the content only. Where it sits (beside the board, or over the page
  as a drawer) is the shell's business, and nothing in here is drawn anew
  when that changes: the scroll position, the search and whatever has the
  focus stay.

  Deleting asks first. Names of categories are asked for in a small dialog.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';

  import BuildingIcon from '@lucide/svelte/icons/building-2';
  import PanelLeftCloseIcon from '@lucide/svelte/icons/panel-left-close';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SearchIcon from '@lucide/svelte/icons/search';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import UsersIcon from '@lucide/svelte/icons/users';
  import XIcon from '@lucide/svelte/icons/x';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardPlace } from '$lib/logic/urls.ts';
  import type { Board } from '$lib/logic/wire.ts';
  import {
    administers,
    boardTrash,
    isGuest,
    peopleOf,
    whileHere,
    type Category,
    type Trash,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Input } from '$lib/ui/input/index.js';

  import { BoardSearch } from './board-search.svelte.ts';
  import { shownTitle } from './shown-title.ts';
  import CategoryNameDialog from './CategoryNameDialog.svelte';
  import PeopleDialog from './PeopleDialog.svelte';
  import SidebarList from './SidebarList.svelte';
  import TrashDialog from './TrashDialog.svelte';
  import WorkspaceSwitcher from './WorkspaceSwitcher.svelte';

  interface Props {
    workspace: Workspace;
    /** The board on the page. */
    boardId: string | undefined;
    boardHref: (board: BoardPlace) => string;
    /** The sidebar is a column beside the board: its button folds it. Otherwise it closes the drawer. */
    beside: boolean;
    onOpenBoard: (board: BoardPlace) => void;
    onClose: () => void;
    onAddBoard: () => void;
    onOpenSettings: () => void;
    /** The address of the organization's screen, for whoever administers it. */
    orgHref?: string | undefined;
    /** Goes to another workspace; with more than one, the head of the sidebar chooses. */
    onSwitchWorkspace?: ((workspaceId: string) => void) | undefined;
  }

  let {
    workspace,
    boardId,
    boardHref,
    beside,
    onOpenBoard,
    onClose,
    onAddBoard,
    onOpenSettings,
    orgHref,
    onSwitchWorkspace,
  }: Props = $props();

  let query = $state('');
  /**
   * What the backend finds for the query beyond what the sidebar shows; only
   * boards of a workspace the reader can be taken to are offered. A guest has
   * the boards they are on in the sidebar already, and the server searches
   * for those who work in the workspace: nothing is asked for them.
   */
  const boardSearch = new BoardSearch(async (text) =>
    isGuest(workspace)
      ? []
      : (await workspace.searchBoards(text)).filter((board) =>
          workspace.workspaces.some((one) => one.id === board.workspaceId),
        ),
  );
  onDestroy(() => {
    boardSearch.stop();
  });

  function setQuery(text: string): void {
    query = text;
    boardSearch.run(text);
  }

  const myName = $derived(
    (workspace.me && workspace.userName(workspace.me.id)) ?? m['centerPanel.unknown-user'](),
  );

  /**
   * Whether "Delete board" is offered for the board that is open: not on a
   * board the reader can only read. What the reader is on the other boards
   * the sidebar does not know; there the server has the say.
   */
  const canDeleteOpenBoard = $derived(
    workspace.board?.boardId === boardId && workspace.board?.canEdit === true,
  );

  // ------------------------------------------------- names of categories

  let naming = $state.raw<{ mode: 'create' } | { mode: 'rename'; category: Category }>({
    mode: 'create',
  });
  let namingOpen = $state(false);

  function submitName(name: string): void {
    if (naming.mode === 'create') void workspace.createCategory(name);
    else if (name !== naming.category.name) void workspace.renameCategory(naming.category.id, name);
  }

  // --------------------------------------------------------------- trash

  /** Managing the people of the workspace, where the backend has that. */
  const access = $derived(peopleOf(workspace));
  let peopleOpen = $state(false);
  let trashOpen = $state(false);
  /** The deleted boards, read anew each time they are shown. */
  let trash = $state.raw<Trash | undefined>(undefined);

  function openTrash(): void {
    trash = boardTrash(workspace);
    trashOpen = true;
    void trash.read();
  }

  // ------------------------------------------------------------ deleting

  type Asked = { kind: 'board'; board: Board } | { kind: 'category'; category: Category };
  let asked = $state.raw<Asked | undefined>(undefined);
  let askOpen = $state(false);
  const askedWiki = $derived(asked?.kind === 'board' && asked.board.kind === 'wiki');

  const titleOf = shownTitle;

  async function deleteBoard(board: Board): Promise<void> {
    const here = await whileHere(workspace, async () => {
      await workspace.deleteBoard(board.id);
      return true;
    });
    // The page showed the board that is gone: show the first one that is left.
    if (!here || board.id !== boardId || workspace.boards.some((left) => left.id === board.id)) {
      return;
    }
    const next = workspace.sidebar.flatMap((entry) => entry.boards)[0];
    if (next) onOpenBoard(next);
  }

  /** Makes a template of the board and opens it, to be changed there. */
  async function templateFromBoard(boardId: string): Promise<void> {
    const made = await whileHere(workspace, () => workspace.createTemplateFromBoard(boardId));
    if (made) onOpenBoard(made);
  }

  function confirmed(): void {
    if (asked?.kind === 'board') void deleteBoard(asked.board);
    else if (asked?.kind === 'category') void workspace.deleteCategory(asked.category.id);
  }
</script>

<div class="flex shrink-0 items-center gap-1 px-2 pt-2 pb-1">
  {#if onSwitchWorkspace && workspace.workspaces.length > 1}
    <WorkspaceSwitcher
      workspaces={workspace.workspaces}
      current={workspace.info?.id}
      onSwitch={onSwitchWorkspace}
    />
  {:else}
    <span class="min-w-0 flex-1 truncate pl-2 text-lg font-semibold tracking-tight" translate="no">
      {m.app_name()}
    </span>
  {/if}
  <IconButton
    label={beside ? m.shell_collapse_sidebar() : m.shell_close_sidebar()}
    data-testid="close-sidebar"
    onclick={onClose}
  >
    {#if beside}<PanelLeftCloseIcon />{:else}<XIcon />{/if}
  </IconButton>
</div>

<div class="relative shrink-0 px-2 py-1">
  <SearchIcon
    class="pointer-events-none absolute top-1/2 left-4.5 size-4 -translate-y-1/2 text-muted-foreground"
    aria-hidden="true"
  />
  <Input
    bind:value={query}
    type="text"
    inputmode="search"
    enterkeyhint="search"
    autocomplete="off"
    autocapitalize="off"
    spellcheck={false}
    aria-label={m.shell_find_board()}
    placeholder={m.shell_find_board()}
    data-testid="sidebar-search"
    oninput={(event: Event) => {
      boardSearch.run((event.currentTarget as HTMLInputElement).value);
    }}
    class="bg-background pr-9 pl-8 roomy:h-touch roomy:pr-12 roomy:text-base"
    onkeydown={(event: KeyboardEvent) => {
      // The first Escape empties the field; only the next one is for what is around it.
      if (event.key !== 'Escape' || query === '') return;
      event.preventDefault();
      setQuery('');
    }}
  />
  {#if query !== ''}
    <IconButton
      label={m.shell_find_board_clear()}
      class="absolute top-1 right-2 text-muted-foreground"
      onclick={() => {
        setQuery('');
      }}
    >
      <XIcon />
    </IconButton>
  {/if}
</div>

<nav class="flex min-h-0 flex-1 flex-col" aria-label={m.shell_boards()}>
  <SidebarList
    {workspace}
    {boardId}
    {boardHref}
    {query}
    found={boardSearch.found}
    searching={boardSearch.pending}
    {canDeleteOpenBoard}
    {onOpenBoard}
    onNewCategory={() => {
      naming = { mode: 'create' };
      namingOpen = true;
    }}
    onRenameCategory={(category: Category) => {
      naming = { mode: 'rename', category };
      namingOpen = true;
    }}
    onDeleteCategory={(category: Category) => {
      asked = { kind: 'category', category };
      askOpen = true;
    }}
    onTemplateFromBoard={(id: string) => void templateFromBoard(id)}
    onDeleteBoard={(board: Board) => {
      asked = { kind: 'board', board };
      askOpen = true;
    }}
  />
</nav>

<div class="shrink-0 border-t p-2 pb-[calc(0.5rem+var(--safe-bottom))]">
  <!-- A guest is on the boards they were invited to: they make none, and bring none back. -->
  {#if !isGuest(workspace)}
    <button
      type="button"
      data-testid="add-board"
      class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
      onclick={onAddBoard}
    >
      <PlusIcon class="size-5 shrink-0 p-0.5" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{m.shell_add_board()}</span>
    </button>
    <button
      type="button"
      data-testid="open-trash"
      aria-haspopup="dialog"
      class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
      onclick={openTrash}
    >
      <Trash2Icon class="size-5 shrink-0 p-0.5" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{m.shell_trash()}</span>
    </button>
  {/if}
  {#if access && !isGuest(workspace) && (administers(workspace) || access.people.teams)}
    <button
      type="button"
      data-testid="open-people"
      aria-haspopup="dialog"
      class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
      onclick={() => {
        peopleOpen = true;
      }}
    >
      <UsersIcon class="size-5 shrink-0 p-0.5" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{m.people_title()}</span>
    </button>
  {/if}
  {#if orgHref}
    <a
      href={orgHref}
      data-testid="open-organization"
      class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
    >
      <BuildingIcon class="size-5 shrink-0 p-0.5" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{m.shell_organization()}</span>
    </a>
  {/if}
  <button
    type="button"
    data-testid="open-settings"
    aria-label={m.shell_account_of({ name: myName })}
    aria-haspopup="dialog"
    class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
    onclick={onOpenSettings}
  >
    <Avatar
      name={myName}
      colorKey={workspace.me?.id}
      locale={workspace.locale}
      decorative
      class="size-5"
    />
    <span class="min-w-0 flex-1 truncate">{myName}</span>
  </button>
</div>

{#if access}
  <PeopleDialog
    bind:open={peopleOpen}
    {workspace}
    people={access.people}
    inviteLink={access.inviteLink}
  />
{/if}

<TrashDialog
  bind:open={trashOpen}
  {trash}
  {workspace}
  title={m.shell_trash()}
  description={m.shell_trash_boards_description()}
  untitled={m['Sidebar.untitled-board']()}
  testId="board-trash"
/>

<CategoryNameDialog
  bind:open={namingOpen}
  mode={naming.mode}
  initial={naming.mode === 'rename' ? naming.category.name : ''}
  onSubmit={submitName}
/>

<ConfirmDialog
  bind:open={askOpen}
  title={asked?.kind === 'category'
    ? m['SidebarCategories.CategoryMenu.DeleteModal.Title']()
    : askedWiki
      ? m.shell_delete_wiki_title()
      : m['DeleteBoardDialog.confirm-tite']()}
  description={asked?.kind === 'category'
    ? m['SidebarCategories.CategoryMenu.DeleteModal.Body']({ categoryName: asked.category.name })
    : asked?.kind === 'board'
      ? askedWiki
        ? m.shell_delete_wiki_info({ title: titleOf(asked.board) })
        : m['DeleteBoardDialog.confirm-info']({ boardTitle: titleOf(asked.board) })
      : undefined}
  confirmLabel={asked?.kind === 'category'
    ? m.shared_delete()
    : m['DeleteBoardDialog.confirm-delete']()}
  destructive
  onConfirm={confirmed}
/>
