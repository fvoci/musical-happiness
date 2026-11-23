<!--
  The sidebar: the name of the app (or, for who works in several workspaces or may
  start an organization, the one shown, to switch to another: `WorkspaceSwitcher.svelte`), a field that narrows the boards down by
  title (and asks the backend for the boards it finds beyond them), the categories with their boards, and at the foot "Add board",
  the trash of boards (`TrashDialog.svelte`), the way to the organization's screen for
  its administrators, and the reader's account (which opens the settings). In the
  reader's own space there are no people to manage and no organization to go to.

  This is the content only. Where it sits (beside the board, or over the page
  as a drawer) is the shell's business, and nothing in here is drawn anew
  when that changes: the scroll position, the search and whatever has the
  focus stay.

  Deleting asks first. Names of categories are asked for in a small dialog.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';

  import BuildingIcon from '@lucide/svelte/icons/building-2';
  import ClockIcon from '@lucide/svelte/icons/clock';
  import NotebookPenIcon from '@lucide/svelte/icons/notebook-pen';
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
  import type { ContainerPlace } from '$lib/logic/urls.ts';
  import type { Container } from '$lib/logic/wire.ts';
  import {
    administers,
    containerTrash,
    firstDiary,
    isGuest,
    isOwnSpace,
    peopleOf,
    shownCourse,
    shownWiki,
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
    /** The board or wiki on the page. */
    containerId: string | undefined;
    meTime?: boolean;
    timeHref?: string | undefined;
    onOpenTime?: (() => void) | undefined;
    containerHref: (container: ContainerPlace) => string;
    /** The sidebar is a column beside the board: its button folds it. Otherwise it closes the drawer. */
    beside: boolean;
    onOpenContainer: (container: ContainerPlace) => void;
    onClose: () => void;
    onAddBoard: () => void;
    onOpenSettings: () => void;
    /** The address of the organization's screen, for whoever administers it. */
    orgHref?: string | undefined;
    /** Goes to another workspace; the head of the sidebar chooses (`WorkspaceSwitcher`). */
    onSwitchWorkspace?: ((workspaceId: string) => void) | undefined;
    /** Starts an organization and goes to its workspace, where the backend can (the switcher offers it). */
    onCreateOrganization?: ((name: string) => Promise<void>) | undefined;
  }

  let {
    workspace,
    containerId,
    meTime = false,
    timeHref,
    onOpenTime,
    containerHref,
    beside,
    onOpenContainer,
    onClose,
    onAddBoard,
    onOpenSettings,
    orgHref,
    onSwitchWorkspace,
    onCreateOrganization,
  }: Props = $props();

  /**
   * The workspace shown, or the one closed to this session in its place; the
   * reader's own space has nobody else in it, and no organization to administer.
   */
  const shown = $derived(workspace.info ?? workspace.closed);
  const ownSpace = $derived(isOwnSpace(shown));

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
  const canDeleteOpenContainer = $derived(
    (workspace.board?.boardId === containerId && workspace.board?.canEdit === true) ||
      (shownWiki(workspace).current?.wikiId === containerId &&
        shownWiki(workspace).current?.canManage === true) ||
      (shownCourse(workspace).current?.courseId === containerId &&
        shownCourse(workspace).current?.canManage === true) ||
      // A diary is the reader's alone.
      workspace.containers.some((one) => one.id === containerId && one.kind === 'diary'),
  );

  /**
   * The way into a diary where the reader's own space has none yet: the
   * first one is made as it is pressed, nothing asked, and opened on today.
   */
  const firstDiaryOffered = $derived(
    ownSpace &&
      workspace.backend.diary !== undefined &&
      !workspace.containers.some((one) => one.kind === 'diary'),
  );

  async function openFirstDiary(): Promise<void> {
    const made = await whileHere(workspace, () => firstDiary(workspace));
    if (made) onOpenContainer(made);
  }

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
    trash = containerTrash(workspace);
    trashOpen = true;
    void trash.read();
  }

  // ------------------------------------------------------------ deleting

  type Asked =
    { kind: 'container'; container: Container } | { kind: 'category'; category: Category };
  let asked = $state.raw<Asked | undefined>(undefined);
  let askOpen = $state(false);
  /** The kind of the board, wiki or diary asked about. */
  const askedKind = $derived(
    asked?.kind === 'container' ? (asked.container.kind ?? 'board') : undefined,
  );

  const titleOf = shownTitle;

  async function deleteContainer(container: Container): Promise<void> {
    const here = await whileHere(workspace, async () => {
      await workspace.deleteContainer(container.id);
      return true;
    });
    // The page showed the container that is gone: show the first one that is left.
    if (
      !here ||
      container.id !== containerId ||
      workspace.containers.some((left) => left.id === container.id)
    ) {
      return;
    }
    const next = workspace.sidebar.flatMap((entry) => entry.containers)[0];
    if (next) onOpenContainer(next);
  }

  /** Makes a template of the board and opens it, to be changed there. */
  async function templateFromBoard(boardId: string): Promise<void> {
    const made = await whileHere(workspace, () => workspace.createTemplateFromBoard(boardId));
    if (made) onOpenContainer(made);
  }

  /** Copies the board or wiki whole and opens the copy. */
  async function duplicateContainer(containerId: string): Promise<void> {
    const made = await whileHere(workspace, () => workspace.duplicateContainer(containerId));
    if (made) onOpenContainer(made);
  }

  function confirmed(): void {
    if (asked?.kind === 'container') void deleteContainer(asked.container);
    else if (asked?.kind === 'category') void workspace.deleteCategory(asked.category.id);
  }
</script>

<div class="flex shrink-0 items-center gap-1 px-2 pt-2 pb-1">
  {#if onSwitchWorkspace && (workspace.workspaces.length > 1 || onCreateOrganization)}
    <WorkspaceSwitcher
      workspaces={workspace.workspaces}
      current={shown?.id}
      onSwitch={onSwitchWorkspace}
      {onCreateOrganization}
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
    {containerId}
    {containerHref}
    {query}
    found={boardSearch.found}
    searching={boardSearch.pending}
    {canDeleteOpenContainer}
    {onOpenContainer}
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
    onDuplicateContainer={(id: string) => void duplicateContainer(id)}
    onDeleteContainer={(container: Container) => {
      asked = { kind: 'container', container };
      askOpen = true;
    }}
  />
</nav>

<div class="shrink-0 border-t p-2 pb-[calc(0.5rem+var(--safe-bottom))]">
  <!-- A guest is on the boards they were invited to: they make none, and bring none back.
       Nor does anybody where no workspace is shown, as in one closed to this session. -->
  {#if workspace.info && !isGuest(workspace)}
    {#if firstDiaryOffered}
      <button
        type="button"
        data-testid="open-diary"
        class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-sidebar-accent roomy:min-h-touch"
        onclick={() => void openFirstDiary()}
      >
        <NotebookPenIcon class="size-5 shrink-0 p-0.5" aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{m.shell_open_diary()}</span>
      </button>
    {/if}
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
  {#if access && !isGuest(workspace) && !ownSpace && (administers(workspace) || access.people.teams)}
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
  {#if timeHref && workspace.me?.id}
    <a
      href={timeHref}
      data-testid="open-my-time"
      aria-current={meTime ? 'page' : undefined}
      class="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-sidebar-accent aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium roomy:min-h-touch"
      onclick={(event: MouseEvent) => {
        if (
          !onOpenTime ||
          event.button !== 0 ||
          event.metaKey ||
          event.ctrlKey ||
          event.shiftKey ||
          event.altKey
        ) {
          return;
        }
        event.preventDefault();
        onOpenTime();
      }}
    >
      <ClockIcon class="size-5 shrink-0 p-0.5" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{m.time_title()}</span>
    </a>
  {/if}
  {#if orgHref && !ownSpace}
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
    : askedKind === 'wiki'
      ? m.shell_delete_wiki_title()
      : askedKind === 'diary'
        ? m.shell_delete_diary_title()
        : askedKind === 'course'
          ? m.shell_delete_course_title()
          : m['DeleteBoardDialog.confirm-tite']()}
  description={asked?.kind === 'category'
    ? m['SidebarCategories.CategoryMenu.DeleteModal.Body']({ categoryName: asked.category.name })
    : asked?.kind === 'container'
      ? askedKind === 'wiki'
        ? m.shell_delete_wiki_info({ title: titleOf(asked.container) })
        : askedKind === 'diary'
          ? m.shell_delete_diary_info({ title: titleOf(asked.container) })
          : askedKind === 'course'
            ? m.shell_delete_course_info({ title: titleOf(asked.container) })
            : m['DeleteBoardDialog.confirm-info']({ boardTitle: titleOf(asked.container) })
      : undefined}
  confirmLabel={asked?.kind === 'category'
    ? m.shared_delete()
    : m['DeleteBoardDialog.confirm-delete']()}
  destructive
  onConfirm={confirmed}
/>
