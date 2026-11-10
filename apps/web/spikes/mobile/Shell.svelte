<!--
  One mock-up: the board page put together the way the address asks for.

  Every screen is the same page. The address decides which proposal each part
  follows and what is open when the page loads; a part the address is not
  about follows the proposal the document recommends.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';

  import { Button } from '$lib/ui/button/index.js';
  import * as Sidebar from '$lib/ui/sidebar/index.js';

  import AppBar from './AppBar.svelte';
  import AppSidebar from './AppSidebar.svelte';
  import BoardSwitcher from './BoardSwitcher.svelte';
  import CardDetail from './CardDetail.svelte';
  import { CURRENT } from './current.ts';
  import { CARDS, OPEN_CARD, VIEWS, type Card, type ViewLink } from './data.ts';
  import Kanban from './Kanban.svelte';
  import { narrow, wide } from './layout.svelte.ts';
  import Table from './Table.svelte';
  import { T } from './text.ts';
  import ViewHeader from './ViewHeader.svelte';

  const { screen, variant, opened } = CURRENT;
  const second = variant === 'b';
  const main = opened === 'main';

  const boards = screen === 'sidebar' && second ? 'switcher' : 'drawer';
  const showsTable = screen === 'table';
  const folding = screen === 'header' && second ? 'b' : 'a';
  const frame = screen === 'card' && second ? 'sheet' : 'page';
  /** The second proposal of the menu screens: a popover even on a narrow screen. */
  const popoversOnly = second && (screen?.startsWith('menu-') ?? false);
  const sheet = $derived(narrow.current && !popoversOnly);

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

  let switcherOpen = $state(main && screen === 'sidebar' && second);
  let card = $state<Card>(OPEN_CARD);
  let cardOpen = $state(
    main && (screen === 'card' || screen === 'menu-select' || screen === 'menu-date'),
  );

  function openCard(chosen: Card): void {
    card = chosen;
    cardOpen = true;
  }
</script>

<Sidebar.Provider class="h-dvh min-h-0 overflow-hidden">
  {#if !(narrow.current && boards === 'switcher')}
    <AppSidebar
      openOnLoad={main && screen === 'sidebar'}
      onfind={() => {
        switcherOpen = true;
      }}
    />
  {/if}
  <Sidebar.Inset class="h-full min-w-0 overflow-hidden pt-(--safe-top)">
    <AppBar
      {boards}
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
  </Sidebar.Inset>
</Sidebar.Provider>

<!--
  "New" within reach of the thumb, in the proposal that takes it out of the
  folded header. Above the home indicator, under every sheet and the card.
-->
{#if !wide.current && folding === 'a'}
  <Button
    aria-label={T.newCard}
    data-testid="new-card-fab"
    class="fixed right-4 bottom-[calc(1rem+var(--safe-bottom))] z-30 size-14 rounded-full shadow-lg"
  >
    <PlusIcon class="size-6" />
  </Button>
{/if}

<BoardSwitcher bind:open={switcherOpen} />

{#key card.id}
  <CardDetail bind:open={cardOpen} {card} {frame} {editing} {sheet} />
{/key}
