<!--
  The card that is open over a board.

  One dialog in three frames: the whole screen on a compact page, a panel at
  the right on a medium one, a dialog in the middle on an expanded one
  (`cardFrame`, lib/layout/presentation.ts). Only the classes of the dialog
  differ (frame.ts). Nothing inside is put together again when the window
  crosses a stage, so the text being typed, the caret, the focus and the
  scroll position stay; and when the address names another card the dialog
  stays too and only what it shows is replaced.

  The card is an address: closing it is the router's business (`onClose`),
  and so is the back button. This component keeps the board store's open
  card in step with the address, and sends what was written to a card before
  letting go of it.
-->
<script lang="ts">
  import { onMount, untrack } from 'svelte';

  import { display } from '$lib/display.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { cardFrame } from '$lib/layout/presentation.ts';
  import { page } from '$lib/layout/room.svelte.ts';
  import type { BoardView } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import * as Dialog from '$lib/ui/dialog/index.js';

  import CardContent from './CardContent.svelte';
  import { FRAME_CLASS } from './frame.ts';

  interface Props {
    board: BoardStore;
    /** The view the card was opened from. */
    view: BoardView | undefined;
    /** The card to show. Undefined: no card is open. */
    cardId: string | undefined;
    onClose: () => void;
    /** Shows another card in place of this one: the copy "Duplicate" made, a card a relation names. */
    onOpenCard?: (cardId: string) => void;
  }

  let { board, view, cardId, onClose, onOpenCard }: Props = $props();

  // The store's open card follows the address. A card that is left is told to
  // store its body first: once closed it sends nothing any more.
  $effect(() => {
    const store = board;
    const id = cardId;
    untrack(() => {
      const left = store.openCard;
      if (left && left.cardId !== id) void left.saveBody();
      if (id) void store.showCard(id);
      else store.closeCard();
    });
  });

  // The tab is closed or put away with something unsaved: send it while there is time.
  onMount(() => {
    const save = (): void => {
      void board.openCard?.saveBody();
    };
    const whenHidden = (): void => {
      if (document.visibilityState === 'hidden') save();
    };
    window.addEventListener('pagehide', save);
    document.addEventListener('visibilitychange', whenHidden);
    return () => {
      window.removeEventListener('pagehide', save);
      document.removeEventListener('visibilitychange', whenHidden);
    };
  });

  const card = $derived(board.openCard?.cardId === cardId ? board.openCard : undefined);
  const frame = $derived(cardFrame(page.stage, display.resolved.cardOpen));
  const title = $derived(card?.summary?.title || m['KanbanCard.untitled']());

  let content = $state<HTMLElement | null>(null);
  /** The card that was shown last, to give the focus back to where it was opened from. */
  let shown: string | undefined;
  $effect(() => {
    if (cardId !== undefined) shown = cardId;
  });

  /** The card whose closing was held back once because its body could not be stored. */
  let held: unknown;

  /**
   * Whether the card may be left, for another card or for the board. What
   * is unsaved is stored first. When that does not work (somebody else wrote
   * the body, the server did not answer), or an image is still going up for
   * the body (shown in its place), the card stays open once, so that nothing
   * is lost by a careless tap. Leaving again, or with the notice already
   * there, leaves.
   */
  async function mayLeave(): Promise<boolean> {
    const open = card;
    // Asked before and after the write: a function, so that the second answer is not taken for the first.
    const unsaved = (): boolean => (open?.dirty ?? false) || (open?.imagesGoingUp ?? false);
    if (open && unsaved() && !open.conflict && held !== open) {
      await open.saveBody();
      if (unsaved() && board.openCard === open) {
        held = open;
        return false;
      }
    }
    return true;
  }

  async function close(): Promise<void> {
    if (await mayLeave()) onClose();
  }

  /** Shows another card in place of this one, when this one may be left. */
  async function openOther(otherId: string): Promise<void> {
    if (await mayLeave()) onOpenCard?.(otherId);
  }

  /**
   * The dialog itself takes the focus, not its first field: a focused field
   * brings up the on-screen keyboard before the card has been read. A card
   * that starts without a title asks for the title itself (CardTitle).
   */
  function focusOnOpen(event: Event): void {
    event.preventDefault();
    if (content && !content.contains(document.activeElement)) content.focus();
  }

  /** Back to the card on the board: the one this was opened from, or the one shown last. */
  function focusOnClose(event: Event): void {
    if (shown === undefined) return;
    const onBoard = document.querySelector<HTMLElement>(`[data-card="${CSS.escape(shown)}"]`);
    if (!onBoard || onBoard.tabIndex < 0) return;
    event.preventDefault();
    onBoard.focus();
  }
</script>

<Dialog.Root
  bind:open={
    () => cardId !== undefined,
    (open) => {
      if (!open) void close();
    }
  }
>
  <Dialog.Content
    bind:ref={content}
    showCloseButton={false}
    data-testid="card-dialog"
    data-frame={frame}
    class={['@container/card flex flex-col gap-0 overflow-hidden p-0', FRAME_CLASS[frame]]}
    onOpenAutoFocus={focusOnOpen}
    onCloseAutoFocus={focusOnClose}
  >
    <Dialog.Title class="sr-only">{title}</Dialog.Title>
    <Dialog.Description class="sr-only">
      {m.card_location({ board: board.board?.title ?? '', view: view?.title ?? '' })}
    </Dialog.Description>
    {#if card}
      {#key card}
        <CardContent
          {board}
          {view}
          {card}
          {frame}
          onClose={() => void close()}
          onOpenCard={(otherId: string) => void openOther(otherId)}
        />
      {/key}
    {/if}
  </Dialog.Content>
</Dialog.Root>
