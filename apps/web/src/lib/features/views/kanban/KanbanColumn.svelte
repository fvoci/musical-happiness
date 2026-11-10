<!--
  One column of the board: a heading and a list of cards that scrolls on its
  own.

  The column is an item of the board's row of columns, dragged by its heading
  (scope "column", lib/drag); its cards are items of the list inside it (no
  scope). While cards are on their way the list shows a gap where they would
  land. The cards themselves stay in the list, hidden, and so does the column
  while a pointer drags it: a finger keeps sending its touch events to the
  element it landed on.

  The width is a matter of the pane, in classes: as wide as the pane less a
  sliver of the neighbours where only one column fits, a fixed width where
  two do.
-->
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';
  import { tick } from 'svelte';

  import CardFace from '$lib/components/card-face/CardFace.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { isComposing } from '$lib/components/keys.ts';
  import OptionChip from '$lib/components/option-chip/OptionChip.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardGroup } from '$lib/logic/group.ts';
  import type { Card, PropertyTemplate } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import { Input } from '$lib/ui/input/index.js';

  import { badgesOf, changeOption, type FaceContext } from './board-data.ts';
  import ColumnMenu from './ColumnMenu.svelte';
  import { columnIdOf, type Activation, type MoveTarget } from './kanban.ts';
  import KanbanCard from './KanbanCard.svelte';

  interface Props {
    board: BoardStore;
    groupBy: PropertyTemplate;
    group: BoardGroup;
    /** The name the column shows: of the option, or of the person. */
    name: string;
    /** The id of the column's box, which its tab names. */
    panelId: string;
    face: FaceContext;
    /** The reader may change the board. */
    editable: boolean;
    /** Cards can be made in, and moved into, this column: its value can be set on a card. */
    settable: boolean;
    /** The columns are options of a select property: they have names, colours and an order. */
    isOption: boolean;
    /** The column can go a place to the left, to the right. Undefined: columns cannot be reordered. */
    canStep: { left: boolean; right: boolean } | undefined;
    noValueName: string;
    /** The other columns, for the menu of a card. */
    columns: readonly MoveTarget[];
    cardHref: (cardId: string) => string;
    /** The cards on their way, wherever they come from. */
    draggedIds: readonly string[];
    /** Where the gap is, counted among the cards without the dragged ones; -1 for no gap. */
    gapAt: number;
    gapHeight: number;
    /** Drawn in the gap instead of an outline: the card the keyboard carries. */
    carried: Card | undefined;
    carriedCount: number;
    selectedIds: readonly string[];
    /** A pointer drags this column: it stays in the document, out of sight. */
    hidden: boolean;
    /** The keyboard carries this column: its heading is the stand-in that holds the focus. */
    lifted: boolean;
    /** The heading is being renamed. */
    renaming: boolean;
    cardHelpId: string;
    columnHelpId: string;
    onActivate: (cardId: string, how: Activation) => void;
    onSelect: ((cardId: string) => void) | undefined;
    onMove: (cardIds: readonly string[], optionId: string) => void;
    onAddCard: (optionId: string) => void;
    onHide: (optionId: string) => void;
    onStep: (optionId: string, step: -1 | 1) => void;
    onRenaming: (optionId: string | undefined) => void;
  }

  let {
    board,
    groupBy,
    group,
    name,
    panelId,
    face,
    editable,
    settable,
    isOption,
    canStep,
    noValueName,
    columns,
    cardHref,
    draggedIds,
    gapAt,
    gapHeight,
    carried,
    carriedCount,
    selectedIds,
    hidden,
    lifted,
    renaming,
    cardHelpId,
    columnHelpId,
    onActivate,
    onSelect,
    onMove,
    onAddCard,
    onHide,
    onStep,
    onRenaming,
  }: Props = $props();

  const option = $derived(group.option);
  /** A column that is an option: the column of cards without a value is not one. */
  const named = $derived(isOption && option.id !== '');
  const orderable = $derived(canStep !== undefined);

  type Entry = { kind: 'card'; card: Card } | { kind: 'gap' };
  const GAP_KEY = ' gap';
  const GAP: Entry = { kind: 'gap' };

  // One entry per card, the same object for as long as the cards are the
  // same: when the gap moves, the list is put together again from these and
  // no card is drawn anew.
  const cardEntries = $derived(
    group.cards.map((card): Entry & { kind: 'card' } => ({ kind: 'card', card })),
  );
  // Read through these, the list below is put together again only when the
  // gap of this column or the dragged cards change, not on every move of a
  // gap in another column.
  const gap = $derived(gapAt);
  const dragged = $derived(draggedIds);

  const entries = $derived.by(() => {
    if (gap < 0 && dragged.length === 0) return cardEntries;
    const list: Entry[] = [];
    let others = 0;
    for (const entry of cardEntries) {
      if (!dragged.includes(entry.card.id)) {
        if (others === gap) list.push(GAP);
        others += 1;
      }
      list.push(entry);
    }
    if (others === gap) list.push(GAP);
    return list;
  });

  const targetsOf = (cardId: string): readonly string[] =>
    selectedIds.includes(cardId) ? selectedIds : [cardId];
  /** The other columns, and this one marked as where the card is. */
  const moveTargets = $derived(
    columns.map((column) => ({ ...column, current: column.optionId === option.id })),
  );

  // ------------------------------------------------------------- renaming

  let draft = $state('');
  let field = $state<HTMLInputElement | null>(null);
  let heading = $state<HTMLElement | null>(null);

  $effect(() => {
    if (!renaming) return;
    draft = option.value;
    void tick().then(() => {
      field?.focus();
      field?.select();
    });
  });

  async function endRename(save: boolean): Promise<void> {
    if (!renaming) return;
    const text = draft.trim();
    onRenaming(undefined);
    if (save && text !== '' && text !== option.value) {
      void changeOption(board, groupBy.id, option.id, { value: text });
    }
    await tick();
    heading?.focus();
  }
</script>

{#snippet label()}
  <h3 id="{panelId}-name" class="flex min-w-0 items-center text-sm font-medium">
    {#if named}
      <OptionChip name={option.value} color={option.color} />
    {:else}
      <span class="truncate" title={name}>{name}</span>
    {/if}
  </h3>
  <span class="shrink-0 text-xs text-muted-foreground" data-column-count>
    <span aria-hidden="true">{group.cards.length}</span>
    <span class="sr-only">{m.kanban_card_count({ count: group.cards.length })}</span>
  </span>
{/snippet}

<section
  id={panelId}
  data-panel
  data-drag-item={orderable ? columnIdOf(option.id) : undefined}
  data-drag-scope={orderable ? 'column' : undefined}
  data-column={option.id}
  aria-labelledby="{panelId}-name"
  class={[
    'flex max-h-full w-full max-w-sm shrink-0 snap-center flex-col rounded-lg bg-muted @columns/pane:w-64 @columns/pane:max-w-none @columns/pane:snap-align-none @header/pane:w-68',
    hidden && 'hidden',
    lifted && 'ring-2 ring-ring',
  ]}
>
  <header class="flex min-h-10 shrink-0 items-center gap-0.5 pt-1 pr-1 pl-1 roomy:min-h-12">
    {#if renaming}
      <Input
        bind:ref={field}
        bind:value={draft}
        aria-label={m.kanban_group_name()}
        autocomplete="off"
        enterkeyhint="done"
        class="mx-1 h-8 min-w-0 flex-1 bg-background roomy:h-10 roomy:text-base"
        onblur={() => void endRename(true)}
        onkeydown={(event: KeyboardEvent) => {
          if (isComposing(event)) return;
          if (event.key === 'Enter') {
            event.preventDefault();
            void endRename(true);
          } else if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            void endRename(false);
          }
        }}
      />
    {:else if orderable}
      <!--
        The heading is the handle of the column where columns can be moved:
        a mouse drags it at once, a finger after resting on it, and Space
        picks the column up from the keyboard.
      -->
      <div
        bind:this={heading}
        data-drag-handle
        data-drag-scope="column"
        data-drag-carried={lifted ? columnIdOf(option.id) : undefined}
        data-column-heading={option.id}
        role="button"
        tabindex="0"
        aria-label={name}
        aria-describedby={columnHelpId}
        class="flex min-h-8 min-w-0 flex-1 cursor-grab items-center gap-2 self-stretch rounded-md pl-2 outline-none select-none [-webkit-touch-callout:none] focus-visible:ring-2 focus-visible:ring-ring"
      >
        {@render label()}
      </div>
    {:else}
      <div
        data-column-heading={option.id}
        class="flex min-h-8 min-w-0 flex-1 items-center gap-2 self-stretch pl-2"
      >
        {@render label()}
      </div>
    {/if}
    {#if editable}
      {#if settable}
        <IconButton
          label={m.kanban_new_card_in({ name })}
          data-column-add={option.id}
          class="shrink-0 text-muted-foreground"
          onclick={() => {
            onAddCard(option.id);
          }}
        >
          <PlusIcon aria-hidden="true" />
        </IconButton>
      {/if}
      <ColumnMenu
        {board}
        {groupBy}
        {option}
        {name}
        isOption={named}
        {canStep}
        {noValueName}
        onHide={() => {
          onHide(option.id);
        }}
        onStep={(step: -1 | 1) => {
          onStep(option.id, step);
        }}
        onRename={() => {
          onRenaming(option.id);
        }}
      />
    {/if}
  </header>

  <div
    role="list"
    aria-labelledby="{panelId}-name"
    data-drag-list={columnIdOf(option.id)}
    data-drag-scroll="y"
    data-column-cards={option.id}
    class="flex min-h-12 flex-col gap-2 overflow-y-auto overscroll-y-contain p-2"
  >
    {#each entries as entry (entry.kind === 'gap' ? GAP_KEY : entry.card.id)}
      {#if entry.kind === 'card'}
        <KanbanCard
          {board}
          card={entry.card}
          {face}
          href={cardHref(entry.card.id)}
          dragged={draggedIds.includes(entry.card.id)}
          selected={selectedIds.includes(entry.card.id)}
          targetIds={targetsOf(entry.card.id)}
          columns={settable ? moveTargets : []}
          describedBy={editable ? cardHelpId : undefined}
          {onActivate}
          {onSelect}
          {onMove}
        />
      {:else if carried}
        {@const badges = badgesOf(carried)}
        <div
          role="listitem"
          data-drag-carried={carried.id}
          tabindex="-1"
          aria-describedby={cardHelpId}
          class="shrink-0 rounded-md outline-none select-none"
        >
          <CardFace
            card={carried}
            templates={face.templates}
            visiblePropertyIds={face.visiblePropertyIds}
            hasBody={badges.hasBody}
            tasksTotal={badges.tasksTotal}
            tasksDone={badges.tasksDone}
            locale={face.locale}
            regional={face.regional}
            users={face.users}
            nameFormat={face.nameFormat}
            lifted
            count={carriedCount}
          />
        </div>
      {:else}
        <div
          data-drag-gap
          aria-hidden="true"
          class="shrink-0 rounded-md border-2 border-dashed border-input-border"
          style:height="{gapHeight}px"
        ></div>
      {/if}
    {/each}
  </div>
  {#if editable && settable}
    <button
      type="button"
      data-column-new={option.id}
      class="mx-2 mb-2 flex min-h-8 shrink-0 items-center gap-1.5 rounded-md px-2 text-left text-sm text-muted-foreground hover:bg-background/60 roomy:min-h-touch"
      onclick={() => {
        onAddCard(option.id);
      }}
    >
      {m['BoardComponent.new']()}
      <span class="sr-only">: {name}</span>
    </button>
  {/if}
</section>
