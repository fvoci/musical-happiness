<!--
  One board: the header of its view, the view itself, and the card that is
  open over it. Which board, view and card comes from the address; this
  component keeps the store and the address in step.

  The parts are features of their own and meet here (src/lib/README.md):
  `view-header`, the views (`kanban`, `table`, `gallery`, `calendar`, `gantt`), and `card`.
-->
<script lang="ts">
  import { tick, untrack } from 'svelte';

  import CardDialog from '$lib/features/card/CardDialog.svelte';
  import ViewHeader from '$lib/features/view-header/ViewHeader.svelte';
  import Calendar from '$lib/features/views/calendar/Calendar.svelte';
  import Gallery from '$lib/features/views/gallery/Gallery.svelte';
  import Gantt from '$lib/features/views/gantt/Gantt.svelte';
  import Kanban from '$lib/features/views/kanban/Kanban.svelte';
  import Table from '$lib/features/views/table/Table.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { propertiesThatMeetFilterGroup } from '$lib/logic/filter.ts';
  import {
    boardPresence,
    newCardMaker,
    referenceTitles,
    whileHere,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import ManagedBoard from './ManagedBoard.svelte';
  import { referencePlaces } from './reference-places.ts';
  import type { Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

  // Where the references in the bodies of this workspace lead: the address bar is here.
  $effect.pre(() => {
    const titles = referenceTitles(workspace);
    const places = referencePlaces(router);
    titles.places = places;
    return () => {
      if (titles.places === places) titles.places = undefined;
    };
  });

  // Open the board the address names.
  $effect(() => {
    const boardId = router.route.boardId;
    if (boardId && workspace.board?.boardId !== boardId) void workspace.openBoard(boardId);
  });

  const board = $derived(
    workspace.board?.boardId === router.route.boardId ? workspace.board : undefined,
  );
  const view = $derived(board?.view(router.route.viewId));

  $effect(() => {
    const shown = board;
    if (shown?.status === 'ready' && !shown.missing && !shown.joinable) {
      return untrack(() => boardPresence(shown).start());
    }
    return undefined;
  });
  $effect(() => {
    if (board) boardPresence(board).setCard(router.route.cardId);
  });

  let page = $state<HTMLElement>();

  /** Joins the open board shown, then puts the focus on the board, where the button was. */
  async function join(): Promise<void> {
    if (!board || !(await board.join())) return;
    await tick();
    page?.focus();
  }

  // An address without a view, or with one that is gone, shows the first view.
  $effect(() => {
    if (board?.status !== 'ready' || view) return;
    const first = board.orderedViews[0];
    if (first) {
      router.go({ ...router.route, viewId: first.id }, { replace: true });
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

  /**
   * A new card that the view will show: made the view's way where it has one
   * (a day on a calendar or a Gantt chart), else with the values the view's
   * filter asks for.
   */
  async function addCard(): Promise<void> {
    if (!board?.board || !view) return;
    const maker = newCardMaker(view.id);
    if (maker) {
      await maker();
      return;
    }
    const properties = propertiesThatMeetFilterGroup(
      view.fields.filter,
      board.board.cardProperties,
    );
    const card = await whileHere(board, () => board.createCard({ properties }));
    if (card) openCard(card.id);
  }
</script>

{#if !board || board.status === 'idle' || board.status === 'loading'}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if board.missing}
  <!-- Not there, or there to be managed only: ManagedBoard tells which. -->
  <ManagedBoard
    {workspace}
    {board}
    {router}
    onJoined={() => {
      void tick().then(() => page?.focus());
    }}
  />
{:else if board.joinable && board.board}
  <div class="grid h-full place-items-center p-6 text-center" data-testid="board-join">
    <div class="flex max-w-md flex-col items-center gap-3">
      <h2 class="text-lg font-medium">
        {board.board.icon ? `${board.board.icon} ` : ''}{board.board.title ||
          m['Sidebar.untitled-board']()}
      </h2>
      {#if board.board.description}
        <p class="whitespace-pre-wrap text-muted-foreground">{board.board.description}</p>
      {/if}
      <p class="text-muted-foreground">{m.app_board_join_hint()}</p>
      <Button aria-disabled={board.joining} aria-busy={board.joining} onclick={() => void join()}
        >{m.app_board_join()}</Button
      >
    </div>
  </div>
{:else if board.status === 'failed'}
  <div class="grid h-full place-items-center p-6 text-center">
    <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
  </div>
{:else if view}
  <div
    bind:this={page}
    class="relative flex h-full min-h-0 flex-col outline-none"
    tabindex="-1"
    data-testid="board-page"
  >
    <ViewHeader
      {board}
      {view}
      bind:search
      viewHref={(viewId: string) => router.href({ ...router.route, viewId, cardId: undefined })}
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
      {:else if view.fields.viewType === 'gantt'}
        <!-- Another Gantt view starts with today in sight: where it is scrolled to is not this one's. -->
        {#key view.id}
          <Gantt {board} {view} {cards} {cardHref} onOpenCard={openCard} />
        {/key}
      {:else}
        <!-- Another calendar view starts at today: its month and day are not this one's. -->
        {#key view.id}
          <Calendar {board} {view} {cards} {cardHref} onOpenCard={openCard} />
        {/key}
      {/if}
    </div>
  </div>
  <CardDialog
    {board}
    {view}
    {cardHref}
    cardId={router.route.cardId}
    onOpenCard={openCard}
    onClose={() => {
      router.closeCard();
    }}
  />
{/if}
