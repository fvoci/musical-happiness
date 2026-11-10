<!--
  One mock-up: the board page put together the way the address asks for.

  Every screen is the same page. The address decides which proposal each part
  follows and what is open when the page loads; a part the address is not
  about follows the proposal the document recommends.

  The page is a row: the list of boards, the pane of the board, and (in one
  proposal, in an expanded window) an open card. What the row is made of
  follows the width of the page (`app.stage`); what the pane looks like
  follows the width the row leaves it (`@container/pane`). Opening the
  sidebar or a card beside the board narrows the pane, and the board in it
  changes as if the window had been made that much smaller.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';

  import { Button } from '$lib/ui/button/index.js';

  import AppBar from './AppBar.svelte';
  import AppSidebar from './AppSidebar.svelte';
  import BoardSwitcher from './BoardSwitcher.svelte';
  import CardDetail from './CardDetail.svelte';
  import { CURRENT } from './current.ts';
  import { CARDS, OPEN_CARD, VIEWS, type Card, type ViewLink } from './data.ts';
  import Kanban from './Kanban.svelte';
  import { app, pane, rem } from './layout.svelte.ts';
  import Table from './Table.svelte';
  import { T } from './text.ts';
  import ViewHeader from './ViewHeader.svelte';

  const { screen, variant, opened } = CURRENT;
  const second = variant === 'b';
  const main = opened === 'main';

  /** The second proposal for the sidebar: no drawer when compact, no strip when medium. */
  const sidebarSecond = screen === 'sidebar' && second;
  const showsTable = screen === 'table';
  const folding = screen === 'header' && second ? 'b' : 'a';
  const frame = screen === 'card' && second ? 'beside' : 'modal';
  /** The second proposal of the menu screens: a popover even in a compact window. */
  const popoversOnly = second && (screen?.startsWith('menu-') ?? false);

  const stage = $derived(app.stage);
  const sheet = $derived(stage === 'compact' && !popoversOnly);

  const FALLBACK_VIEW: ViewLink = { id: '', name: '', kind: 'kanban' };
  const view =
    VIEWS.find((entry) => entry.kind === (showsTable ? 'table' : 'kanban')) ?? FALLBACK_VIEW;

  const headerOpened =
    screen === 'menu-view' && main
      ? 'settings'
      : screen === 'header' && (opened === 'more' || opened === 'views')
        ? opened
        : 'none';
  const editing =
    main && screen === 'menu-select'
      ? 'status'
      : main && screen === 'menu-date'
        ? 'due'
        : undefined;
  /** The first card of the column in view: there is room under it for a popover. */
  const menuCard = main && screen === 'menu-card' ? CARDS[0]?.id : undefined;

  let switcherOpen = $state(main && sidebarSecond && app.stage !== 'medium');
  let card = $state<Card>(OPEN_CARD);
  let cardOpen = $state(
    main && (screen === 'card' || screen === 'menu-select' || screen === 'menu-date'),
  );
  /** The list of boards is out over the page (compact and medium). */
  let drawerOpen = $state(
    main &&
      screen === 'sidebar' &&
      (app.stage === 'medium' || (app.stage === 'compact' && !second)),
  );
  /** The list of boards is a column of the page and not a strip (expanded). */
  let pinned = $state(!CURRENT.sidebarAsStrip && !(screen === 'sidebar' && !main));

  /**
   * A card beside the board takes half the page (within the limits CardDetail
   * gives its pane). When the fixed sidebar, 16rem, would then leave the board
   * less than 20rem, one column and its margins, the sidebar gives way and is
   * a strip for as long as the card is open.
   */
  const cardBeside = $derived(cardOpen && frame === 'beside' && stage === 'expanded');
  const crowded = $derived(
    cardBeside &&
      app.width - rem(16) - Math.min(rem(40), Math.max(rem(22.5), app.width / 2)) < rem(20),
  );

  const sidebarForm = $derived(
    stage === 'expanded'
      ? pinned && !crowded
        ? 'fixed'
        : 'strip'
      : stage === 'medium' && !sidebarSecond
        ? 'strip'
        : 'none',
  );
  /**
   * Whether the list can slide in over the page, and so whether anything
   * opens it that way. Not where it is a column of the page already; but
   * where a card has pushed that column down to a strip, yes.
   */
  const overlay = $derived(
    stage === 'medium' || (stage === 'compact' && !sidebarSecond) || crowded,
  );
  const leading = $derived(
    stage === 'compact' && sidebarSecond
      ? 'switcher'
      : sidebarForm === 'fixed'
        ? 'toggle'
        : sidebarForm === 'strip'
          ? 'none'
          : 'menu',
  );

  // A drawer that was out when the window grew has nothing left to cover:
  // the list is on the page. It does not come back out when the window shrinks.
  $effect(() => {
    if (!overlay) drawerOpen = false;
  });

  function openCard(chosen: Card): void {
    card = chosen;
    cardOpen = true;
  }

  function openList(): void {
    if (stage === 'expanded' && !crowded) pinned = !pinned;
    else drawerOpen = true;
  }
</script>

<div
  data-testid="app"
  data-stage={stage}
  class="@container/app flex h-[calc(100dvh-var(--safe-top))] min-h-0 overflow-hidden"
>
  <AppSidebar
    form={sidebarForm}
    {overlay}
    bind:drawerOpen
    onexpand={openList}
    onfind={() => {
      switcherOpen = true;
    }}
  />

  <main
    data-testid="pane"
    class="@container/pane relative flex min-w-0 flex-1 flex-col overflow-hidden bg-background"
    {@attach pane.attach}
  >
    <AppBar
      {leading}
      onmenu={openList}
      onswitch={() => {
        switcherOpen = true;
      }}
    />
    <ViewHeader {folding} {view} opened={headerOpened} {sheet} />
    <div class="min-h-0 flex-1">
      {#if showsTable}
        <Table narrowAs={second ? 'list' : 'table'} scrollX={CURRENT.scrollX} onopen={openCard} />
      {:else}
        <Kanban
          paging={screen === 'kanban' && second ? 'free' : 'snap'}
          column={CURRENT.column}
          {menuCard}
          {sheet}
          onopen={openCard}
        />
      {/if}
    </div>

    <!--
      "New" within reach of the thumb, in the proposal that takes it out of
      the folded header. In the corner of the pane, not of the window: beside
      an open card it stays with the board. Above the home indicator.
    -->
    {#if pane.measured && !pane.header && folding === 'a'}
      <Button
        aria-label={T.newCard}
        data-testid="new-card-fab"
        class="absolute right-4 bottom-[calc(1rem+var(--safe-bottom))] z-30 size-14 rounded-full shadow-lg"
      >
        <PlusIcon class="size-6" />
      </Button>
    {/if}
  </main>

  {#key card.id}
    <CardDetail bind:open={cardOpen} {card} {frame} {editing} {sheet} />
  {/key}
</div>

<BoardSwitcher bind:open={switcherOpen} />
