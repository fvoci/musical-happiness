<!--
  One card in a column: the face, what makes it a thing to open, select and
  drag, and its menu laid over the corner.

  The card is a link to its own address: a press opens it. With Ctrl or ⌘
  the press takes the card into or out of the selection, with Shift it
  selects the range up to it. Enter does the same from the keyboard, Space
  picks the card up (lib/drag). What a plain press means while cards are
  being selected is the board's to say (`onActivate`).

  The card stays in the document while it is dragged, hidden: a finger that
  drags it keeps sending its touch events to this element. The menu is a
  sibling of the link, not inside it: a button in a link is no button to a
  screen reader.
-->
<script lang="ts">
  import { primaryModifier } from '$lib/keys.ts';
  import CardFace from '$lib/components/card-face/CardFace.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Card } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';

  import { badgesOf, type FaceContext } from './board-data.ts';
  import CardMenu from './CardMenu.svelte';
  import type { Activation, MoveTarget } from './kanban.ts';

  interface Props {
    board: BoardStore;
    card: Card;
    face: FaceContext;
    href: string;
    /** On its way with a drag: hidden where it was. */
    dragged: boolean;
    selected: boolean;
    /** The cards the menu's "move" and "delete" act on. */
    targetIds: readonly string[];
    /** The columns the card can be moved to; empty when it cannot. */
    columns: readonly MoveTarget[];
    /** The id of the text that says how to drag with the keyboard. */
    describedBy: string | undefined;
    onActivate: (cardId: string, how: Activation) => void;
    /** Takes the card into or out of the selection. Undefined: cards cannot be selected. */
    onSelect: ((cardId: string) => void) | undefined;
    onMove: (cardIds: readonly string[], optionId: string) => void;
  }

  let {
    board,
    card,
    face,
    href,
    dragged,
    selected,
    targetIds,
    columns,
    describedBy,
    onActivate,
    onSelect,
    onMove,
  }: Props = $props();

  const uid = $props.id();
  const badges = $derived(badgesOf(card));

  function activate(event: MouseEvent | KeyboardEvent): void {
    if (primaryModifier(event)) onActivate(card.id, 'toggle');
    else if (event.shiftKey) onActivate(card.id, 'range');
    else onActivate(card.id, 'open');
  }

  function onClick(event: MouseEvent): void {
    const target = event.target;
    // The link of a URL property and its buttons keep their own presses.
    if (target instanceof Element && target.closest('button, input, a[href]')) return;
    activate(event);
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== 'Enter' || event.target !== event.currentTarget) return;
    event.preventDefault();
    activate(event);
  }
</script>

<div role="listitem" class={['group/card relative shrink-0', dragged && 'hidden']}>
  <div
    role="link"
    data-drag-item={card.id}
    data-card={card.id}
    data-selected={selected ? '' : undefined}
    tabindex="0"
    aria-labelledby="{uid}-title"
    aria-describedby={describedBy}
    class="cursor-pointer rounded-md outline-none select-none [-webkit-touch-callout:none] focus-visible:ring-2 focus-visible:ring-ring"
    onclick={onClick}
    onkeydown={onKeyDown}
  >
    <CardFace
      {card}
      templates={face.templates}
      visiblePropertyIds={face.visiblePropertyIds}
      hasBody={badges.hasBody}
      tasksTotal={badges.tasksTotal}
      tasksDone={badges.tasksDone}
      titleId="{uid}-title"
      locale={face.locale}
      regional={face.regional}
      users={face.users}
      {selected}
      menuRoom
    />
    {#if selected}
      <span class="sr-only">{m.kanban_selected()}</span>
    {/if}
  </div>
  <div data-slot="card-menu" class="absolute top-1 right-1 roomy:top-0 roomy:right-0">
    <CardMenu
      {board}
      {card}
      {href}
      {targetIds}
      {columns}
      {selected}
      onSelect={onSelect &&
        (() => {
          onSelect(card.id);
        })}
      onMove={(optionId: string) => {
        onMove(targetIds, optionId);
      }}
    />
  </div>
</div>
