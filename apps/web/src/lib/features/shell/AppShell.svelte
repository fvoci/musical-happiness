<!--
  The frame of the workspace: the sidebar, the top bar, and the pane the page
  is drawn in.

  The page is a row. What the row is made of follows the stage of the page
  (lib/layout/presentation.ts, `sidebarShape`), and what is in the pane
  follows the width the row leaves it (`@container/pane`, `pane`):

  - compact: the pane alone. The sidebar is a drawer over the page, opened
    with the button of the top bar and closed by a tap outside, by choosing a
    board, by Escape and by the back button.
  - medium: a strip of 3.5rem and the pane. The strip opens the same drawer,
    over the page: the board is not pushed aside.
  - expanded: the sidebar, 16rem, fixed beside the pane. Folded, it is the
    strip, and the pane gets the room; the device remembers that.

  The three are one element in three places, not three components: the list
  of boards is never drawn anew when the stage changes, so its scroll
  position, its search and its focus stay. A drawer that is out stays out
  from compact to medium and back; when the sidebar becomes fixed there is
  nothing left to cover and the drawer is over, and it does not come back out
  when the page shrinks again (question 34 of docs/mobile-proposal.md).

  The page box (#app) is already clear of the top, left and right edges the
  device keeps. The drawer is laid over the whole screen, so it clears the
  top and the left itself; everything that reaches the bottom edge clears
  that.
-->
<script module lang="ts">
  import type { Workspace } from '$lib/state/index.ts';

  /** What the shell lends the page beside it. */
  export interface PaneTools {
    workspace: Workspace;
    /** Opens the picker of what to make; none for a guest, who makes nothing. */
    addBoard: (() => void) | undefined;
  }
</script>

<script lang="ts">
  import { tick, type Snippet } from 'svelte';

  import { nameOfWorkspace } from '$lib/components/workspace-name.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { ContainerPlace } from '$lib/logic/urls.ts';
  import type { Container } from '$lib/logic/wire.ts';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { focusRevealed } from '$lib/layout/focus.ts';
  import { sidebarShape } from '$lib/layout/presentation.ts';
  import { paneCovered } from '$lib/layout/over-pane.svelte.ts';
  import { page, pane } from '$lib/layout/room.svelte.ts';
  import {
    createCourse,
    createCourseFromTemplate,
    createWiki,
    isGuest,
    isOwnSpace,
    whileHere,
    type CourseDraft,
    type WikiDraft,
  } from '$lib/state/index.ts';
  import type { AuditRecord } from '$lib/state/organization.ts';

  import AuditDialog from './AuditDialog.svelte';
  import { devReadout } from './dev.svelte.ts';
  import DevReadout from './DevReadout.svelte';
  import { shellLayout } from './layout.ts';
  import SettingsDialog from './SettingsDialog.svelte';
  import { SidebarState } from './sidebar-state.svelte.ts';
  import Sidebar from './Sidebar.svelte';
  import SidebarStrip from './SidebarStrip.svelte';
  import NewCourseDialog from './NewCourseDialog.svelte';
  import NewWikiDialog from './NewWikiDialog.svelte';
  import TemplatePicker from './TemplatePicker.svelte';
  import TopBar from './TopBar.svelte';
  import Transfers from './Transfers.svelte';
  import { undoKey } from './undo-keys.ts';

  interface Props {
    workspace: Workspace;
    /** The board or wiki that is shown, to mark it in the sidebar. */
    containerId: string | undefined;
    /** Whether the personal "My time" screen is shown. */
    meTime?: boolean;
    /** The address of the personal "My time" screen, when time tracking is available. */
    timeHref?: string | undefined;
    onOpenTime?: (() => void) | undefined;
    /** The address of a board or a wiki, for a link to it. */
    containerHref: (container: ContainerPlace) => string;
    /** The address of a board's or a wiki's public link with its read token. */
    sharedHref: (containerId: string, readToken: string) => string;
    onOpenContainer: (container: ContainerPlace) => void;
    /** The languages the app has, as tags, for the settings. */
    locales?: readonly string[];
    /** The reader's language setting: `auto`, or the one of `locales` it is shown in. */
    language?: string;
    /** The one of `locales` that `auto` shows the app in. */
    autoLocale?: string;
    /** Changes the language setting (`auto` or one of `locales`); the page is loaded again in it. */
    onChooseLanguage?: (setting: string) => void;
    /** Signs the reader out. Without it (no accounts: the backend in memory) it is not offered. */
    onSignOut?: () => void;
    /** Changes the reader's password. Without it (no accounts) it is not offered. */
    onChangePassword?: (oldPassword: string, newPassword: string) => Promise<void>;
    /** What the app adds to the account's part of the settings: where features meet. */
    accountSettings?: Snippet;
    /** Whether the settings are open: the app opens them for what it has to show there. */
    settingsOpen?: boolean;
    /** The address of the organization's screen, for whoever administers it. */
    orgHref?: string;
    /** Goes to another workspace of the reader's. */
    onSwitchWorkspace?: (workspaceId: string) => void;
    /** Starts an organization and goes to its workspace, where the backend can. */
    onCreateOrganization?: (name: string) => Promise<void>;
    /** Reads the workspace's records. Without it (no server) they are not offered. */
    readRecords?: (workspaceId: string) => Promise<AuditRecord[]>;
    /** The buttons of the search and of the notifications, in the top bar; none without them. */
    search?: Snippet<[Workspace]>;
    /** The page: what is shown beside the sidebar, given the way to the picker of what to make. */
    children: Snippet<[PaneTools]>;
  }

  let {
    workspace,
    containerId,
    meTime = false,
    timeHref,
    onOpenTime,
    containerHref,
    sharedHref,
    onOpenContainer,
    locales,
    language,
    autoLocale,
    onChooseLanguage,
    onSignOut,
    onChangePassword,
    accountSettings,
    settingsOpen = $bindable(false),
    orgHref,
    onSwitchWorkspace,
    onCreateOrganization,
    readRecords,
    search,
    children,
  }: Props = $props();

  devReadout.start(location.search);

  const sidebar = new SidebarState();
  const stage = $derived(page.stage);
  const layout = $derived(shellLayout(sidebarShape(stage), sidebar.collapsed));
  /** The drawer is out over the page. */
  const drawerOut = $derived(layout.drawer && sidebar.drawerOpen);

  let shell = $state<HTMLElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  let recordsOpen = $state(false);
  /** The reader works in the workspace shown (no guest of it): its records are theirs to read. */
  const worksHere = $derived(workspace.workspaces.some((one) => one.id === workspace.info?.id));

  // Where the page has no drawer (the sidebar is fixed, or folded on an
  // expanded page) an open one is over, and stays so.
  $effect(() => {
    if (!layout.drawer) sidebar.drawerOpen = false;
  });

  closeOnBack(
    () => drawerOut,
    () => {
      sidebar.drawerOpen = false;
    },
  );

  // While the drawer is out the focus is in it (the rest of the page is
  // inert), on the drawer itself and not on its search field: a focused field
  // would bring up the keyboard. Afterwards it goes back to what opened it.
  let opener: Element | null = null;
  $effect(() => {
    if (!drawerOut) return;
    focusRevealed(panel, { preventScroll: true });
    return () => {
      const from = opener;
      opener = null;
      const lost = document.activeElement;
      if (!(from instanceof HTMLElement) || !from.isConnected) return;
      if (lost === null || lost === document.body || panel?.contains(lost)) focusRevealed(from);
    };
  });

  /**
   * The button pressed is what the focus goes back to: Safari does not focus a
   * button that is clicked, so the focused element may be the page itself.
   */
  function openDrawer(event: Event): void {
    opener = event.currentTarget instanceof Element ? event.currentTarget : document.activeElement;
    sidebar.drawerOpen = true;
  }

  async function focusOn(testId: string): Promise<void> {
    await tick();
    focusRevealed(shell?.querySelector<HTMLElement>(`[data-testid="${testId}"]`));
  }

  /** The strip's first button: the whole list, as a drawer or, on an expanded page, back in its column. */
  function expand(event: Event): void {
    if (layout.drawer) {
      openDrawer(event);
    } else {
      sidebar.setCollapsed(false);
      void focusOn('close-sidebar');
    }
  }

  /** The button at the top of the list: folds the fixed sidebar, or closes the drawer. */
  function closeList(): void {
    if (layout.list === 'beside') {
      sidebar.setCollapsed(true);
      void focusOn('expand-sidebar');
    } else {
      sidebar.drawerOpen = false;
    }
  }

  /** A board or wiki was chosen. From the drawer, the router puts the new address in place of the drawer's entry. */
  function openContainer(container: ContainerPlace): void {
    onOpenContainer(container);
    sidebar.drawerOpen = false;
  }

  function openTime(): void {
    onOpenTime?.();
    sidebar.drawerOpen = false;
  }

  /**
   * Opens the settings; a drawer they were asked for from closes first.
   *
   * Not both in one step. The drawer gives its entry of the history back
   * when it closes, and the browser does that a moment later; an entry the
   * dialog added in the meantime would be counted from the wrong place, and
   * closing the dialog would then leave the page. So the dialog opens once
   * the drawer's step back has arrived.
   */
  function openSettings(): void {
    if (!drawerOut) {
      settingsOpen = true;
      return;
    }
    const open = (): void => {
      window.removeEventListener('popstate', open);
      clearTimeout(fallback);
      settingsOpen = true;
    };
    // The step back always arrives, within a frame or two; the timer is for a
    // browser that loses it, and long enough not to get ahead of a slow one.
    const fallback = setTimeout(open, 2000);
    window.addEventListener('popstate', open);
    sidebar.drawerOpen = false;
  }

  /** "Add board" asks: an empty board, or one from a template. */
  let picking = $state(false);

  function addBoard(): void {
    picking = true;
  }

  /**
   * An empty board with one view (the workspace makes the two together,
   * because a board without a view has nothing to show), and the page goes
   * to it.
   */
  async function addEmptyBoard(): Promise<void> {
    const made = await whileHere(workspace, () =>
      workspace.createBoard({ title: '' }, m['View.NewBoardTitle'](), m.shell_new_board_status()),
    );
    if (made) openContainer(made);
  }

  /** "New wiki" asks for its name and who finds it. */
  let namingWiki = $state(false);

  async function addWiki(draft: WikiDraft): Promise<void> {
    const made = await whileHere(workspace, () => createWiki(workspace, draft));
    if (made) openContainer(made);
  }

  /** "New course" asks for its name and (in a shared workspace) who finds it. */
  let namingCourse = $state(false);

  async function addCourse(draft: CourseDraft, templateId?: string): Promise<void> {
    const made = await whileHere(workspace, () =>
      templateId
        ? createCourseFromTemplate(workspace, templateId, draft)
        : createCourse(workspace, draft),
    );
    if (made) openContainer(made);
  }

  async function addBoardFromTemplate(template: Container): Promise<void> {
    const made = await whileHere(workspace, () => workspace.duplicateContainer(template.id));
    if (made) openContainer(made);
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' && drawerOut && !event.defaultPrevented) {
      // Only an Escape pressed in the drawer: a menu over it has its own.
      if (event.target instanceof Node && panel?.contains(event.target)) {
        event.preventDefault();
        sidebar.drawerOpen = false;
      }
      return;
    }
    const asked = undoKey(event);
    if (!asked) return;
    event.preventDefault();
    if (asked === 'undo') void workspace.undoLast();
    else void workspace.redoLast();
  }
</script>

<svelte:window {onkeydown} />

<div
  bind:this={shell}
  data-testid="app-shell"
  data-stage={stage}
  data-sidebar={layout.list === 'beside' ? 'fixed' : drawerOut ? 'drawer' : 'closed'}
  class="flex h-[calc(100dvh-var(--safe-top))] min-h-0 overflow-hidden"
>
  <SidebarStrip
    {workspace}
    {containerId}
    {meTime}
    {timeHref}
    onOpenTime={onOpenTime ? openTime : undefined}
    {containerHref}
    hidden={!layout.strip}
    inert={drawerOut || paneCovered()}
    onOpenContainer={openContainer}
    onExpand={expand}
    onAddBoard={addBoard}
    onOpenSettings={openSettings}
  />

  {#if drawerOut}
    <button
      type="button"
      tabindex="-1"
      data-testid="sidebar-scrim"
      aria-label={m.shell_close_sidebar()}
      class="fixed inset-0 z-40 animate-in cursor-default scrim duration-200 fade-in-0"
      onclick={() => {
        sidebar.drawerOpen = false;
      }}
    ></button>
  {/if}

  <!--
    The list of boards. Beside the board it is a column of the row. Otherwise
    it lies over the page at the left edge of the screen, slid out of sight
    until it is the drawer; hidden that way and not taken out of the layout,
    so that it keeps its scroll position. It is visible from the first moment
    of sliding in (it takes the focus then) and hidden only once it has slid
    out.
  -->
  <div
    bind:this={panel}
    data-testid="sidebar"
    role={drawerOut ? 'dialog' : undefined}
    aria-modal={drawerOut ? 'true' : undefined}
    aria-label={m.ui_sidebar_title()}
    tabindex="-1"
    inert={layout.list === 'over' && !drawerOut}
    class={[
      'flex flex-col bg-sidebar text-sidebar-foreground outline-none',
      layout.list === 'beside'
        ? 'w-64 shrink-0 border-r'
        : 'fixed inset-y-0 left-0 z-40 w-[calc(min(18rem,75vw)+var(--safe-left))] border-r pt-(--safe-top) pl-(--safe-left) shadow-lg',
      layout.list === 'over' &&
        (drawerOut
          ? '[transition:translate_200ms_ease-out]'
          : 'invisible -translate-x-full [transition:translate_200ms_ease-in,visibility_0s_200ms]'),
    ]}
  >
    <Sidebar
      {workspace}
      {containerId}
      {meTime}
      {timeHref}
      onOpenTime={onOpenTime ? openTime : undefined}
      {containerHref}
      beside={layout.list === 'beside'}
      onOpenContainer={openContainer}
      onClose={closeList}
      onAddBoard={addBoard}
      onOpenSettings={openSettings}
      {orgHref}
      {onSwitchWorkspace}
      {onCreateOrganization}
    />
  </div>

  <!-- Focusable by a script: where the focus goes when what had it is gone. -->
  <main
    class="@container/pane flex min-w-0 flex-1 flex-col outline-none"
    tabindex="-1"
    inert={drawerOut}
    {@attach pane.attach}
  >
    <TopBar
      {workspace}
      {containerId}
      {meTime}
      {sharedHref}
      menuButton={layout.menuButton}
      onOpenSidebar={openDrawer}
      {search}
      inert={paneCovered()}
    />
    <div class="relative min-h-0 flex-1">
      {@render children({ workspace, addBoard: isGuest(workspace) ? undefined : addBoard })}
    </div>
  </main>
</div>

<Transfers {workspace} />

<TemplatePicker
  bind:open={picking}
  {workspace}
  onNewWiki={workspace.backend.wiki
    ? () => {
        namingWiki = true;
      }
    : undefined}
  onNewCourse={workspace.backend.course
    ? () => {
        namingCourse = true;
      }
    : undefined}
  onEmptyBoard={() => void addEmptyBoard()}
  onFromTemplate={(template: Container) => void addBoardFromTemplate(template)}
  onEditTemplate={(template: Container) => {
    openContainer(template);
  }}
/>

<NewWikiDialog
  bind:open={namingWiki}
  workspaceName={nameOfWorkspace(workspace.info, m.shell_workspace_unnamed)}
  alone={isOwnSpace(workspace.info)}
  onCreate={(draft: WikiDraft) => void addWiki(draft)}
/>

<NewCourseDialog
  bind:open={namingCourse}
  {workspace}
  workspaceName={nameOfWorkspace(workspace.info, m.shell_workspace_unnamed)}
  alone={isOwnSpace(workspace.info)}
  onCreate={(draft: CourseDraft, templateId?: string) => void addCourse(draft, templateId)}
/>

<SettingsDialog
  bind:open={settingsOpen}
  {workspace}
  {locales}
  {language}
  {autoLocale}
  {onChooseLanguage}
  {onSignOut}
  {onChangePassword}
  account={accountSettings}
  {containerHref}
  onOpenContainer={(container: ContainerPlace) => {
    settingsOpen = false;
    openContainer(container);
  }}
  onOpenRecords={readRecords && worksHere
    ? () => {
        settingsOpen = false;
        recordsOpen = true;
      }
    : undefined}
/>

{#if readRecords}
  <AuditDialog
    bind:open={recordsOpen}
    workspaceId={workspace.info?.id}
    locale={workspace.locale}
    {readRecords}
  />
{/if}

{#if devReadout.shown}
  <DevReadout />
{/if}
