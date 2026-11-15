<!--
  The strip: what is left of the sidebar where the whole list does not stand
  beside the board (a medium page, or an expanded one whose sidebar the
  reader folded). One button a board, in the order of the sidebar, so that
  another board is one tap away and the one on the page is marked. Above
  them the button that brings the whole list out; below them "Add board" and
  the reader's account.

  A board is drawn as its icon, or without one as the first character of its
  title. When there are more boards than fit, the boards scroll and the
  buttons above and below stay.
-->
<script lang="ts">
  import PanelLeftOpenIcon from '@lucide/svelte/icons/panel-left-open';
  import PlusIcon from '@lucide/svelte/icons/plus';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardPlace } from '$lib/logic/urls.ts';
  import { isGuest, type Workspace } from '$lib/state/index.ts';

  import { shownTitle } from './shown-title.ts';
  import BoardIcon from './BoardIcon.svelte';

  interface Props {
    workspace: Workspace;
    /** The board on the page. */
    boardId: string | undefined;
    boardHref: (board: BoardPlace) => string;
    /** Out of the layout: the list stands beside the board, or the page is compact. */
    hidden: boolean;
    /** Behind the open drawer: neither pressed nor reached with the keyboard. */
    inert: boolean;
    onOpenBoard: (board: BoardPlace) => void;
    onExpand: () => void;
    onAddBoard: () => void;
    onOpenSettings: () => void;
  }

  let {
    workspace,
    boardId,
    boardHref,
    hidden,
    inert,
    onOpenBoard,
    onExpand,
    onAddBoard,
    onOpenSettings,
  }: Props = $props();

  const boards = $derived(workspace.sidebar.flatMap((entry) => entry.boards));
  const titleOf = shownTitle;
  const myName = $derived(
    (workspace.me && workspace.userName(workspace.me.id)) ?? m['centerPanel.unknown-user'](),
  );

  function open(event: MouseEvent, board: BoardPlace): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    onOpenBoard(board);
  }
</script>

<nav
  data-testid="sidebar-strip"
  aria-label={m.shell_boards()}
  {inert}
  class={[
    'w-14 shrink-0 flex-col items-center gap-1 border-r bg-sidebar pt-2 pb-[calc(0.5rem+var(--safe-bottom))] text-sidebar-foreground',
    hidden ? 'hidden' : 'flex',
  ]}
>
  <IconButton label={m.shell_expand_sidebar()} data-testid="expand-sidebar" onclick={onExpand}>
    <PanelLeftOpenIcon />
  </IconButton>
  <div class="my-1 w-8 shrink-0 border-t"></div>
  <ul
    class="flex min-h-0 w-full flex-1 [scrollbar-width:none] flex-col items-center gap-1 overflow-y-auto overscroll-contain py-1"
  >
    {#each boards as board (board.id)}
      {@const title = titleOf(board)}
      <li class="shrink-0">
        <a
          href={boardHref(board)}
          aria-label={board.kind === 'wiki' ? m.shell_wiki_named({ title }) : title}
          {title}
          aria-current={board.id === boardId ? 'page' : undefined}
          class="grid size-8 place-items-center rounded-lg hover:bg-sidebar-accent aria-[current=page]:bg-sidebar-accent aria-[current=page]:ring-1 aria-[current=page]:ring-input-border roomy:size-touch"
          onclick={(event) => {
            open(event, board);
          }}
        >
          <BoardIcon icon={board.icon} kind={board.kind} {title} locale={workspace.locale} />
        </a>
      </li>
    {/each}
  </ul>
  <div class="my-1 w-8 shrink-0 border-t"></div>
  <!-- A guest is on the boards they were invited to, and makes none. -->
  {#if !isGuest(workspace)}
    <IconButton label={m.shell_add_board()} data-testid="strip-add-board" onclick={onAddBoard}>
      <PlusIcon />
    </IconButton>
  {/if}
  <IconButton
    label={m.shell_account_of({ name: myName })}
    aria-haspopup="dialog"
    data-testid="strip-open-settings"
    onclick={onOpenSettings}
  >
    <Avatar
      name={myName}
      colorKey={workspace.me?.id}
      locale={workspace.locale}
      decorative
      class="size-6"
    />
  </IconButton>
</nav>
