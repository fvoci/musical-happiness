<!--
  One board: the header of its view, the view itself, and the card that is
  open over it. Which board, view and card comes from the address; this
  component keeps the store and the address in step.

  The parts are features of their own and meet here (src/lib/README.md):
  `view-header`, the views (`kanban`, `table`, `gallery`), and `card`.
-->
<script lang="ts">
  import CardDialog from '$lib/features/card/CardDialog.svelte';
  import ViewHeader from '$lib/features/view-header/ViewHeader.svelte';
  import Gallery from '$lib/features/views/gallery/Gallery.svelte';
  import Kanban from '$lib/features/views/kanban/Kanban.svelte';
  import Table from '$lib/features/views/table/Table.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { propertiesThatMeetFilterGroup } from '$lib/logic/filter.ts';
  import type { Workspace } from '$lib/state/index.ts';

  import type { Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

  // Open the board the address names.
  $effect(() => {
    const boardId = router.route.boardId;
    if (boardId && workspace.board?.boardId !== boardId) void workspace.openBoard(boardId);
  });

  const board = $derived(
    workspace.board?.boardId === router.route.boardId ? workspace.board : undefined,
  );
  const view = $derived(board?.view(router.route.viewId));

  // An address without a view, or with one that is gone, shows the first view.
  $effect(() => {
    if (board?.status !== 'ready' || view) return;
    const first = board.orderedViews[0];
    if (first) {
      router.go(
        { boardId: board.boardId, viewId: first.id, cardId: router.route.cardId },
        { replace: true },
      );
    }
  });

  /** What the reader typed into the search of the view. It is theirs alone and not stored. */
  let search = $state('');
  // Another view starts without a search. By its identifier: the view is a
  // new object after every change to it, and a change is not another view.
  const viewId = $derived(view?.id);
  $effect(() => {
    if (viewId) search = '';
  });

  const cards = $derived(board && view ? board.cardsOf(view, search) : []);

  const cardHref = (cardId: string): string => router.href({ ...router.route, cardId });
  const openCard = (cardId: string): void => {
    router.openCard(cardId);
  };

  /** A new card that the view will show: it gets the values the view's filter asks for. */
  async function addCard(): Promise<void> {
    if (!board?.board || !view) return;
    const properties = propertiesThatMeetFilterGroup(
      view.fields.filter,
      board.board.cardProperties,
    );
    const card = await board.createCard({ properties });
    if (card) openCard(card.id);
  }
</script>

{#if !board || board.status === 'idle' || board.status === 'loading'}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if board.missing}
  <div class="grid h-full place-items-center p-6 text-center">
    <div>
      <p class="text-lg font-medium" role="alert">{m['error.board-not-found']()}</p>
      <p class="mt-1 text-muted-foreground">{m.app_board_not_found_hint()}</p>
    </div>
  </div>
{:else if board.status === 'failed'}
  <div class="grid h-full place-items-center p-6 text-center">
    <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
  </div>
{:else if view}
  <div class="relative flex h-full min-h-0 flex-col" data-testid="board-page">
    <ViewHeader
      {board}
      {view}
      bind:search
      viewHref={(viewId: string) => router.href({ boardId: board.boardId, viewId })}
      onOpenView={(viewId: string) => {
        router.openView(viewId);
      }}
      onAddCard={() => void addCard()}
    />
    <div class="min-h-0 flex-1">
      {#if view.fields.viewType === 'board'}
        <Kanban {board} {view} {cards} {cardHref} onOpenCard={openCard} />
      {:else if view.fields.viewType === 'table'}
        <Table {board} {view} {cards} {cardHref} onOpenCard={openCard} />
      {:else if view.fields.viewType === 'gallery'}
        <Gallery {board} {view} {cards} {cardHref} onOpenCard={openCard} />
      {:else}
        <div class="grid h-full place-items-center p-6 text-center">
          <p class="max-w-sm text-muted-foreground">{m.app_calendar_not_built()}</p>
        </div>
      {/if}
    </div>
  </div>
  <CardDialog
    {board}
    {view}
    cardId={router.route.cardId}
    onClose={() => {
      router.closeCard();
    }}
  />
{/if}
