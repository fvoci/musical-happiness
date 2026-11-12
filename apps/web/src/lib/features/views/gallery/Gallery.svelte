<!--
  The cards of a view as a grid.

  How many cards stand side by side follows the width of the box the gallery
  is in: as many as fit at the least width of a card (`MIN_CARD`, in the
  class below), and one where not even that fits. Nothing else changes with
  the width.

  A card shows the same face as on a board, larger: icon and title, the
  first picture of its body and the beginning of its text, the properties
  the view turned on, how its tasks stand. The previews are read only while
  a gallery is shown (`CardPreviews`), and a picture is fetched once its
  card comes near the screen.

  The order is the view's. Cards are not dragged here: the grid runs along
  two axes and lib/drag sorts along one. A sort of the view orders them.
-->
<script lang="ts">
  import CardFace from '$lib/components/card-face/CardFace.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { propertiesThatMeetFilterGroup } from '$lib/logic/filter.ts';

  import { badgesOf, faceContextOf } from '../kanban/board-data.ts';
  import CardMenu from '../kanban/CardMenu.svelte';
  import type { ViewProps } from '../props.ts';
  import { CardPreviews, whileHere } from '$lib/state/index.ts';

  let { board, view, cards, cardHref, onOpenCard }: ViewProps = $props();

  const uid = $props.id();
  const face = $derived(faceContextOf(board, view));
  const previews = $derived(new CardPreviews(board.workspace.backend, board.boardId));

  /** The address of a picture of the board; none where the backend has no bytes for it. */
  function imageOf(fileId: string | undefined): string | undefined {
    return fileId ? board.workspace.backend.fileUrl(board.boardId, fileId) : undefined;
  }

  /** A new card that the view will show: it gets the values the view's filter asks for. */
  async function addCard(): Promise<void> {
    if (!board.board) return;
    const properties = propertiesThatMeetFilterGroup(
      view.fields.filter,
      board.board.cardProperties,
    );
    const card = await whileHere(board, () => board.createCard({ properties }));
    if (card) onOpenCard(card.id);
  }

  function onClick(event: MouseEvent, cardId: string): void {
    const target = event.target;
    // The link of a URL property and its buttons keep their own presses.
    if (target instanceof Element && target.closest('button, input, a[href]')) return;
    onOpenCard(cardId);
  }
</script>

<!-- The previews are read and followed while the gallery is on screen. -->
<div class="h-full overflow-y-auto" data-testid="gallery" {@attach () => previews.start()}>
  <!-- MIN_CARD is the 14rem below: a card is never narrower, unless the box itself is. -->
  <ul
    aria-label={m.gallery_cards()}
    data-testid="gallery-grid"
    class="grid grid-cols-[repeat(auto-fill,minmax(min(100%,14rem),1fr))] gap-3 p-3 pb-[calc(0.75rem+var(--safe-bottom))] @header/pane:gap-4 @header/pane:p-4 @header/pane:pb-[calc(1rem+var(--safe-bottom))]"
  >
    {#each cards as card (card.id)}
      {@const badges = badgesOf(card)}
      {@const preview = previews.of(card.id)}
      <li class="group/card relative min-w-0">
        <div
          role="link"
          data-card={card.id}
          tabindex="0"
          aria-labelledby="{uid}-{card.id}"
          class="h-full cursor-pointer rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onclick={(event) => {
            onClick(event, card.id);
          }}
          onkeydown={(event) => {
            if (event.key !== 'Enter' || event.target !== event.currentTarget) return;
            event.preventDefault();
            onOpenCard(card.id);
          }}
        >
          <CardFace
            {card}
            templates={face.templates}
            visiblePropertyIds={face.visiblePropertyIds}
            hasBody={badges.hasBody}
            tasksTotal={badges.tasksTotal}
            tasksDone={badges.tasksDone}
            size="large"
            previewImage={imageOf(preview?.imageFileId)}
            previewText={preview?.text}
            titleId="{uid}-{card.id}"
            locale={face.locale}
            regional={face.regional}
            users={face.users}
            menuRoom
            class="h-full"
          />
        </div>
        <div data-slot="card-menu" class="absolute top-2 right-2 roomy:top-1 roomy:right-1">
          <CardMenu {board} {card} href={cardHref(card.id)} />
        </div>
      </li>
    {/each}
    {#if board.canEdit}
      <li class="min-w-0">
        <button
          type="button"
          data-testid="gallery-new"
          class="flex h-full min-h-14 w-full items-center justify-center rounded-md border border-dashed border-input-border px-4 text-sm text-muted-foreground hover:bg-muted roomy:min-h-touch"
          onclick={() => void addCard()}
        >
          {m['TableComponent.plus-new']()}
        </button>
      </li>
    {/if}
  </ul>
</div>
