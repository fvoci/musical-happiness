<!--
  A board for the invited only that the reader, an administrator of the
  workspace, manages without reading: in place of its content, its title, a
  line saying that joining shows the content and that every member reads
  that it was joined, the button that joins it (as a viewer), and the way to
  its members (the share dialog, without the public link, which is content).
  Anything else not there is `NotFound`.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import ShareDialog from '$lib/features/shell/ShareDialog.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { joinManaged, readManagedOnly } from '$lib/state/managed-board.ts';
  import type { BoardStore, Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import NotFound from './NotFound.svelte';
  import type { Router } from './router.svelte.ts';

  interface Props {
    workspace: Workspace;
    board: BoardStore;
    router: Router;
    /** The board is joined and shown: the page takes the focus, where the button was. */
    onJoined: () => void;
  }

  let { workspace, board, router, onJoined }: Props = $props();

  let managed = $state<'asking' | 'yes' | 'no'>('asking');
  let joining = $state(false);
  let membersOpen = $state(false);

  // Asked once for the board shown: is it there to be managed?
  $effect(() => {
    const store = board;
    managed = 'asking';
    // What the reading looks at besides the store is not what this waits for.
    void untrack(() => readManagedOnly(store)).then(
      (yes) => {
        if (store === board) managed = yes ? 'yes' : 'no';
      },
      () => {
        if (store === board) managed = 'no';
      },
    );
  });

  async function join(): Promise<void> {
    if (joining) return;
    joining = true;
    const joined = await joinManaged(board);
    joining = false;
    if (joined) onJoined();
  }
</script>

{#if managed === 'asking'}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if managed === 'no' || !board.board}
  <NotFound {router} />
{:else}
  <div class="grid h-full place-items-center p-6 text-center" data-testid="board-managed">
    <div class="flex max-w-md flex-col items-center gap-3">
      <h2 class="text-lg font-medium">
        {board.board.icon ? `${board.board.icon} ` : ''}{board.board.title ||
          m['Sidebar.untitled-board']()}
      </h2>
      <p class="text-muted-foreground">{m.app_board_managed_hint()}</p>
      <div class="flex flex-wrap justify-center gap-2">
        <Button aria-disabled={joining} aria-busy={joining} onclick={() => void join()}
          >{m.app_board_managed_join()}</Button
        >
        <Button
          variant="outline"
          aria-haspopup="dialog"
          onclick={() => {
            membersOpen = true;
          }}>{m.app_board_managed_members()}</Button
        >
      </div>
    </div>
  </div>
  <ShareDialog
    bind:open={membersOpen}
    {workspace}
    {board}
    linkOf={(readToken: string) =>
      new URL(router.href({ boardId: board.boardId, readToken }), location.href).href}
    membersOnly
  />
{/if}
