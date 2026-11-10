<!-- A stand-in until the card detail is built: the title and the body, read only. -->
<script lang="ts">
  import BodyView from '$lib/editor/BodyView.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardView } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import * as Dialog from '$lib/ui/dialog/index.js';

  interface Props {
    board: BoardStore;
    /** The view the card was opened from. */
    view: BoardView | undefined;
    /** The card to show. Undefined: no card is open. */
    cardId: string | undefined;
    onClose: () => void;
  }

  let { board, view, cardId, onClose }: Props = $props();

  $effect(() => {
    if (cardId) void board.showCard(cardId);
    else board.closeCard();
  });

  const card = $derived(board.openCard?.cardId === cardId ? board.openCard : undefined);
</script>

<Dialog.Root
  open={cardId !== undefined}
  onOpenChange={(open) => {
    if (!open) onClose();
  }}
>
  <Dialog.Content data-testid="card-dialog">
    <Dialog.Header>
      <Dialog.Title>{card?.summary?.title || m['KanbanCard.untitled']()}</Dialog.Title>
      <Dialog.Description>{view?.title}</Dialog.Description>
    </Dialog.Header>
    {#if card?.status === 'ready'}
      <BodyView
        doc={card.body}
        resolveUser={(userId: string) => board.workspace.userName(userId)}
      />
    {/if}
  </Dialog.Content>
</Dialog.Root>
