<!-- A stand-in until the shell is built: the boards as a list beside the page. -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { pane } from '$lib/layout/room.svelte.ts';
  import type { Workspace } from '$lib/state/index.ts';

  interface Props {
    workspace: Workspace;
    /** The board that is shown, to mark it in the sidebar. */
    boardId: string | undefined;
    /** The address of a board, for a link to it. */
    boardHref: (boardId: string) => string;
    onOpenBoard: (boardId: string) => void;
    /** The page: what is shown beside the sidebar. */
    children: Snippet;
  }

  let { workspace, boardId, boardHref, onOpenBoard, children }: Props = $props();
</script>

<div class="flex h-dvh" data-testid="app-shell">
  <nav
    class="w-64 shrink-0 overflow-y-auto border-r bg-sidebar p-2"
    aria-label={m.ui_sidebar_title()}
  >
    {#each workspace.sidebar as entry (entry.category.id)}
      <p class="px-2 pt-3 pb-1 text-xs font-medium text-muted-foreground">{entry.category.name}</p>
      <ul>
        {#each entry.boards as board (board.id)}
          <li>
            <a
              class="block truncate rounded-md px-2 py-1.5 hover:bg-sidebar-accent aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium"
              href={boardHref(board.id)}
              aria-current={board.id === boardId ? 'page' : undefined}
              onclick={(event) => {
                event.preventDefault();
                onOpenBoard(board.id);
              }}
            >
              {board.icon}
              {board.title || m['Sidebar.untitled-board']()}
            </a>
          </li>
        {/each}
      </ul>
    {/each}
  </nav>
  <main class="@container/pane min-w-0 flex-1" {@attach pane.attach}>
    {@render children()}
  </main>
</div>
