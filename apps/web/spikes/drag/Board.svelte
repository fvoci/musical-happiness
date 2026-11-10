<!--
  The spike's page: a board of made-up cards. Cards are dragged between and
  within the columns, and the columns themselves are dragged by their
  headings: two sortables on one root, told apart by their scope.

  The order of the columns and of their cards is the state; a drop replaces
  it, and nothing else ever changes it. Three switches turn on what a plain
  board does not have: a sorted view (only the column can be chosen), a
  selection (several cards dragged as one) and a hidden column (a place to
  drop on that is no list).
-->
<script lang="ts">
  import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';

  import { Sortable, type SortableMove } from '$lib/drag/sortable.svelte.ts';
  import { moveItems, type ListOrder } from '$lib/logic/drop-target.ts';
  import { Label } from '$lib/ui/label/index.js';
  import { Switch } from '$lib/ui/switch/index.js';

  import CardFace from './CardFace.svelte';
  import Column from './Column.svelte';
  import { byTask, fakeBoard, startFrom, type Card } from './data.ts';
  import {
    COLUMN_INSTRUCTIONS,
    COLUMN_MESSAGES,
    INSTRUCTIONS,
    MESSAGES,
    THINGS_TO_TRY,
  } from './messages.ts';

  /** The one list of the columns' sortable: the row of columns. */
  const ROW = 'board';
  /** The hidden column, which is a zone and not a list. */
  const ICEBOX = 'icebox';

  const start = startFrom(location.search);
  const board = fakeBoard(start.sizes);
  const titles = new Map(board.columns.map((column) => [column.id, column.title]));
  const compare = byTask(board.cards);
  if (start.rtl) document.documentElement.dir = 'rtl';

  /** The columns in the order they stand in, each with its cards in the order they were put in. */
  let order = $state.raw<readonly ListOrder[]>(board.order);
  /** The cards of the hidden column. */
  let icebox = $state.raw<readonly string[]>([]);
  let sorted = $state(start.sorted);
  let selecting = $state(start.selecting);
  let zoneShown = $state(start.zone);
  let selected = $state.raw<readonly string[]>([]);
  // Shown at the foot of the page, for a person trying it and for the browser tests.
  let moves = $state(0);
  let lastMove = $state('');
  let opened = $state('');

  /** What the page shows: in the sorted view every column orders its cards itself. */
  const shown = $derived(
    sorted ? order.map((list) => ({ ...list, items: [...list.items].sort(compare) })) : order,
  );
  const row = $derived<ListOrder[]>([{ id: ROW, items: order.map((list) => list.id) }]);

  function describe({ itemId, itemIds, from, to }: SortableMove): string {
    const more = itemIds.length > 1 ? ` (+${itemIds.length - 1})` : '';
    const target =
      to.kind === 'index'
        ? `${to.listId} ${to.index}`
        : to.kind === 'list'
          ? `${to.listId}, sorted`
          : `zone ${to.zoneId}`;
    return `${itemId}${more}: ${from.listId} ${from.index} to ${target}`;
  }

  const cards = new Sortable({
    order: () => shown,
    onDrop: (move) => {
      const { itemIds, to } = move;
      if (to.kind === 'index') {
        order = moveItems(order, itemIds, to);
      } else if (to.kind === 'list') {
        // The sorted view shows them where its order puts them; underneath they go to the end.
        order = moveItems(order, itemIds, { listId: to.listId, index: Infinity });
      } else {
        order = order.map((list) => ({
          ...list,
          items: list.items.filter((id) => !itemIds.includes(id)),
        }));
        icebox = [...icebox, ...itemIds];
      }
      // A selection that was dropped has done its work; one that stayed behind stays.
      if (selected.includes(move.itemId)) selected = [];
      moves += 1;
      lastMove = describe(move);
    },
    messages: MESSAGES,
    itemLabel: (id) => board.cards[id]?.title ?? id,
    listLabel: (id) => titles.get(id) ?? id,
    zoneLabel: () => 'Icebox',
    placement: () => (sorted ? 'list' : 'free'),
    sortedIndex: (itemId, listId, carried) =>
      shown
        .find((list) => list.id === listId)
        ?.items.filter((id) => !carried.includes(id) && compare(id, itemId) < 0).length,
    selection: () => (selecting ? selected : []),
    zones: () => (zoneShown ? [ICEBOX] : []),
  });

  const columns = new Sortable({
    scope: 'column',
    axis: 'x',
    order: () => row,
    onDrop: ({ itemId, itemIds, from, to }) => {
      if (to.kind !== 'index') return;
      const ids = moveItems(row, itemIds, to)[0]?.items ?? [];
      order = ids.flatMap((id) => order.find((list) => list.id === id) ?? []);
      moves += 1;
      lastMove = `column ${itemId}: ${from.index} to ${to.index}`;
    },
    messages: COLUMN_MESSAGES,
    itemLabel: (id) => titles.get(id) ?? id,
  });

  const cardDrag = $derived(cards.active);
  const columnDrag = $derived(columns.active);
  const draggedCard = $derived(cardDrag ? board.cards[cardDrag.itemId] : undefined);
  const draggedColumn = $derived(
    columnDrag ? shown.find((list) => list.id === columnDrag.itemId) : undefined,
  );
  const notice = $derived(cards.notice || columns.notice);

  type ColumnEntry = { kind: 'column'; list: ListOrder } | { kind: 'gap' };

  /**
   * The columns as drawn. A pointer dragging a column leaves it where it is,
   * hidden, and a gap shows where it would land. The keyboard has no overlay
   * to show the column in, so there the column itself stands at the place it
   * would land.
   */
  const columnEntries = $derived.by((): ColumnEntry[] => {
    const gap = columnDrag?.gap;
    if (!columnDrag || !gap) return shown.map((list) => ({ kind: 'column', list }));
    if (columnDrag.mode === 'keyboard') {
      const ids = moveItems(row, columnDrag.itemIds, gap)[0]?.items ?? [];
      return ids.flatMap((id): ColumnEntry[] => {
        const list = shown.find((candidate) => candidate.id === id);
        return list ? [{ kind: 'column', list }] : [];
      });
    }
    const entries: ColumnEntry[] = [];
    let others = 0;
    for (const list of shown) {
      if (!columnDrag.itemIds.includes(list.id)) {
        if (others === gap.index) entries.push({ kind: 'gap' });
        others += 1;
      }
      entries.push({ kind: 'column', list });
    }
    if (others === gap.index) entries.push({ kind: 'gap' });
    return entries;
  });

  function cardsOf(list: ListOrder | undefined): Card[] {
    return (list?.items ?? []).flatMap((id) => board.cards[id] ?? []);
  }

  /** A click, or Enter, on a card: opens it, or while selecting, takes it into or out of the selection. */
  function activate(card: Card): void {
    if (!selecting) {
      opened = card.id;
    } else if (selected.includes(card.id)) {
      selected = selected.filter((id) => id !== card.id);
    } else {
      selected = [...selected, card.id];
    }
  }

  /** Brings the cards of the hidden column back, at the end of the first column. */
  function restore(): void {
    const first = order[0];
    if (!first) return;
    order = order.map((list) =>
      list === first ? { ...list, items: [...list.items, ...icebox] } : list,
    );
    icebox = [];
  }
</script>

<div class="flex h-dvh flex-col">
  <header class="border-b px-3 py-2">
    <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
      <h1 class="text-base font-semibold">Drag spike</h1>
      <div class="flex items-center gap-2">
        <Switch id="switch-sorted" bind:checked={sorted} />
        <Label for="switch-sorted">Sorted view</Label>
      </div>
      <div class="flex items-center gap-2">
        <Switch
          id="switch-select"
          bind:checked={selecting}
          onCheckedChange={() => {
            selected = [];
          }}
        />
        <Label for="switch-select">Select cards</Label>
      </div>
      <div class="flex items-center gap-2">
        <Switch id="switch-zone" bind:checked={zoneShown} />
        <Label for="switch-zone">Hidden column</Label>
      </div>
    </div>
    <details open class="mt-2 text-xs text-muted-foreground">
      <summary class="cursor-pointer select-none">What to try</summary>
      <ul class="mt-1 list-disc ps-4 sm:columns-2 sm:gap-x-8 lg:columns-3">
        {#each THINGS_TO_TRY as thing (thing)}
          <li>{thing}</li>
        {/each}
      </ul>
    </details>
    <p id="drag-help" class="sr-only">{INSTRUCTIONS}</p>
    <p id="column-help" class="sr-only">{COLUMN_INSTRUCTIONS}</p>
  </header>

  <div
    data-testid="board"
    data-drag-list={ROW}
    data-drag-scope="column"
    data-drag-scroll="x"
    data-dragging={cardDrag?.mode ?? columnDrag?.mode}
    data-drop-list={cardDrag?.gap?.listId}
    data-drop-index={cardDrag?.gap?.index}
    data-drop-kind={cardDrag ? (cardDrag.to?.kind ?? 'outside') : undefined}
    data-column-index={columnDrag?.gap?.index}
    data-outside={(cardDrag ?? columnDrag)?.outside ? '' : undefined}
    class={[
      'flex min-h-0 flex-1 items-start gap-3 overflow-x-auto overflow-y-hidden p-3',
      (cardDrag ?? columnDrag)?.mode === 'pointer' && 'cursor-grabbing',
    ]}
    {@attach cards.attach}
    {@attach columns.attach}
  >
    {#each columnEntries as entry (entry.kind === 'gap' ? ' gap' : entry.list.id)}
      {#if entry.kind === 'column'}
        {@const id = entry.list.id}
        <Column
          column={{ id, title: titles.get(id) ?? id }}
          cards={cardsOf(entry.list)}
          draggedIds={cardDrag?.itemIds ?? []}
          gapAt={cardDrag?.gap?.listId === id ? cardDrag.gap.index : -1}
          gapHeight={cardDrag?.height ?? 0}
          carried={cardDrag?.mode === 'keyboard' ? draggedCard : undefined}
          carriedCount={cardDrag?.itemIds.length ?? 1}
          selectedIds={selected}
          hidden={columnDrag?.mode === 'pointer' && columnDrag.itemId === id}
          lifted={columnDrag?.mode === 'keyboard' && columnDrag.itemId === id}
          describedBy="drag-help"
          headingDescribedBy="column-help"
          onactivate={activate}
        />
      {:else}
        <div
          data-column-gap
          aria-hidden="true"
          class="shrink-0 rounded-lg border-2 border-dashed border-input-border"
          style:width="{columnDrag?.width ?? 0}px"
          style:height="{columnDrag?.height ?? 0}px"
        ></div>
      {/if}
    {/each}

    {#if zoneShown}
      {@const over = cardDrag?.to?.kind === 'zone'}
      <aside
        data-drag-zone={ICEBOX}
        data-testid="zone"
        data-over={over ? '' : undefined}
        aria-label="Icebox, a hidden column"
        class={[
          'flex w-56 shrink-0 flex-col gap-2 self-stretch rounded-lg border-2 border-dashed p-3',
          over ? 'border-primary bg-accent' : 'border-input-border',
        ]}
      >
        <p class="font-semibold">Icebox</p>
        <p class="text-xs text-muted-foreground">
          A hidden column, holding <span data-testid="zone-count">{icebox.length}</span>
          {icebox.length === 1 ? 'card' : 'cards'}. Drop cards here to put them away.
        </p>
        {#if over && cardDrag.mode === 'keyboard' && draggedCard}
          <div
            data-drag-carried={draggedCard.id}
            role="button"
            tabindex="0"
            aria-describedby="drag-help"
            class="rounded-md select-none"
          >
            <CardFace card={draggedCard} lifted count={cardDrag.itemIds.length} />
          </div>
        {/if}
        {#if icebox.length > 0}
          <button
            type="button"
            class="self-start rounded-md border bg-background px-2 py-1 text-xs"
            onclick={restore}
          >
            Put them back
          </button>
        {/if}
      </aside>
    {/if}
  </div>

  <footer class="flex flex-wrap gap-x-4 border-t px-3 py-2 text-xs text-muted-foreground">
    <span>Moves: <span data-testid="moves">{moves}</span></span>
    <span>Last move: <span data-testid="last-move">{lastMove || 'none'}</span></span>
    <span>Opened: <span data-testid="opened">{opened || 'none'}</span></span>
    {#if selecting}
      <span>Selected: <span data-testid="selected">{selected.length}</span></span>
    {/if}
  </footer>

  <div data-testid="announcer" class="sr-only" aria-live="assertive" aria-atomic="true">
    {cards.announcement}
  </div>
  <div data-testid="column-announcer" class="sr-only" aria-live="assertive" aria-atomic="true">
    {columns.announcement}
  </div>
</div>

{#if notice}
  <!-- What a release would do when that is not plain to see. Above the finger, at the top of the page. -->
  <div
    data-testid="notice"
    class="pointer-events-none fixed inset-x-0 top-2 z-60 flex justify-center px-3"
  >
    <span
      class="rounded-full bg-foreground px-3 py-1 text-center text-sm font-medium text-background shadow-lg"
    >
      {notice}
    </span>
  </div>
{/if}

{#if cardDrag?.mode === 'pointer' && cards.overlay && draggedCard}
  <!-- Under the pointer, above everything, and never in the way of what is beneath. -->
  <div
    data-drag-overlay={draggedCard.id}
    aria-hidden="true"
    class={[
      'pointer-events-none fixed top-0 left-0 z-50 rotate-1 will-change-transform',
      cardDrag.outside && 'opacity-50',
    ]}
    style:width="{cardDrag.width}px"
    style:translate="{cards.overlay.x}px {cards.overlay.y}px"
  >
    <CardFace card={draggedCard} lifted count={cardDrag.itemIds.length} />
  </div>
{/if}

{#if columnDrag?.mode === 'pointer' && columns.overlay && draggedColumn}
  <!-- The column under the pointer: its heading and as many cards as fit its height. -->
  <div
    data-drag-overlay={draggedColumn.id}
    aria-hidden="true"
    class={[
      'pointer-events-none fixed top-0 left-0 z-50 rotate-1 overflow-hidden rounded-lg bg-muted shadow-lg ring-2 ring-ring will-change-transform',
      columnDrag.outside && 'opacity-50',
    ]}
    style:width="{columnDrag.width}px"
    style:height="{columnDrag.height}px"
    style:translate="{columns.overlay.x}px {columns.overlay.y}px"
  >
    <div class="flex items-center gap-1 px-2 pt-3 pb-1">
      <GripVerticalIcon class="size-4 shrink-0 text-muted-foreground" />
      <span class="min-w-0 flex-1 font-semibold">{titles.get(draggedColumn.id)}</span>
      <span class="pe-1 text-xs text-muted-foreground">{draggedColumn.items.length}</span>
    </div>
    <div class="flex flex-col gap-2 p-2">
      {#each cardsOf(draggedColumn).slice(0, 12) as card (card.id)}
        <CardFace {card} />
      {/each}
    </div>
  </div>
{/if}
