<!--
  What is inside the frame of an open card, top to bottom: the top row, then
  one box that scrolls (title, properties, body, attachments, comments), and
  the field for a new comment as the last row, always in sight.

  The component is made anew for another card and for nothing else: not for
  a change of the frame, and not for anything that arrives from the board.

  The title, icon and properties come from the board's list of cards and are
  there at once; the body, attachments and comments are read when the card is
  opened. A card that is not there (the address is wrong, or somebody deleted
  it, also while it was open) says so and can be closed.
-->
<script lang="ts">
  import { onDestroy, untrack } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { CardFrame } from '$lib/layout/presentation.ts';
  import type { BoardView } from '$lib/logic/wire.ts';
  import type { BoardStore, CardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { BodySaver } from './body-saver.svelte.ts';
  import CardAttachments from './CardAttachments.svelte';
  import CardBody from './CardBody.svelte';
  import CardComments from './CardComments.svelte';
  import CardHeader from './CardHeader.svelte';
  import CardProperties from './CardProperties.svelte';
  import CardTitle from './CardTitle.svelte';
  import CommentComposer from './CommentComposer.svelte';
  import { keepScroll } from './scroll-keeper.ts';

  interface Props {
    board: BoardStore;
    view: BoardView | undefined;
    card: CardStore;
    frame: CardFrame;
    onClose: () => void;
    onOpenCard?: (cardId: string) => void;
  }

  let { board, view, card, frame, onClose, onOpenCard }: Props = $props();

  const saver = new BodySaver(untrack(() => card));
  onDestroy(() => {
    saver.dispose();
  });

  const summary = $derived(card.summary);
  const ready = $derived(card.status === 'ready');
  /** A card that starts without a title and a body was just made: the caret goes to its title. */
  const blank = untrack(() => card.summary?.title === '' && !card.summary.hasBody);

  let scroller = $state<HTMLElement>();
  /** The reader is deleting the card: it closes as soon as it is gone. */
  let deleting = $state(false);

  // The app tells of a conflict with a line at the bottom of the screen, over
  // the comment field. The card says the same above its body, with the two
  // choices: the line is taken away.
  $effect(() => {
    if (!card.conflict) return;
    untrack(() => {
      for (const problem of board.workspace.problems) {
        if (problem.action === 'conflict') board.workspace.dismissProblem(problem.id);
      }
    });
  });

  // What these two need is taken before they wait: by the time a write has
  // answered, this component may be gone (the card was deleted, or changed
  // for another), and its arguments with it.
  async function duplicate(): Promise<void> {
    const [store, cardId, open] = [board, card.cardId, onOpenCard];
    // The copy is made from what is stored: store what was typed first.
    await saver.flush();
    const copy = await store.duplicateCard(cardId);
    if (copy) open?.(copy.id);
  }

  async function remove(): Promise<void> {
    const [store, cardId, close] = [board, card.cardId, onClose];
    deleting = true;
    await store.deleteCards([cardId]);
    // Refused: the card is still there, and stays open.
    if (store.card(cardId)) deleting = false;
    else close();
  }

  function showNewest(): void {
    requestAnimationFrame(() => {
      scroller?.scrollTo({ top: scroller.scrollHeight });
    });
  }
</script>

<CardHeader
  {board}
  {view}
  {card}
  {frame}
  saveState={saver.state}
  {onClose}
  onDuplicate={() => void duplicate()}
  onDelete={() => void remove()}
/>

{#if deleting}
  <div class="flex-1"></div>
{:else if card.missing || (card.status === 'failed' && !summary)}
  <div class="grid flex-1 place-items-center p-6 text-center" data-testid="card-missing">
    <div class="flex max-w-sm flex-col items-center gap-2">
      <p class="text-lg font-medium" role="alert">
        {card.missing ? m['CardDialog.nocard']() : m.app_problem_load()}
      </p>
      {#if card.missing}
        <p class="text-muted-foreground">{m.card_missing_hint()}</p>
      {/if}
      <Button class="mt-2 roomy:min-h-touch" onclick={onClose}>{m.ui_close()}</Button>
    </div>
  </div>
{:else if !summary}
  <div class="grid flex-1 place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.card_loading()}</p>
  </div>
{:else}
  <div
    bind:this={scroller}
    class="min-h-0 flex-1 overflow-y-auto overscroll-contain"
    data-testid="card-scroll"
    {@attach keepScroll}
  >
    <article
      class="mx-auto flex w-full max-w-3xl flex-col gap-6 px-4 pt-2 pb-8 @card-roomy/card:px-8"
    >
      {#if summary.fields.isTemplate}
        <p class="rounded-md bg-muted px-3 py-2 text-muted-foreground">
          {m['CardDialog.editing-template']()}
        </p>
      {/if}
      <CardTitle {board} card={summary} autofocus={blank} />
      <CardProperties {board} {view} card={summary} />
      {#if ready}
        <CardBody {board} {card} {saver} />
        <CardAttachments {board} {card} />
        <CardComments {board} {card} />
      {:else if card.status === 'failed'}
        <p class="border-t pt-5 text-muted-foreground" role="alert">{m.app_problem_load()}</p>
      {:else}
        <p class="border-t pt-5 text-muted-foreground" role="status">{m.card_loading()}</p>
      {/if}
    </article>
  </div>
  {#if ready && board.canComment}
    <CommentComposer {board} {card} onSent={showNewest} />
  {/if}
{/if}
