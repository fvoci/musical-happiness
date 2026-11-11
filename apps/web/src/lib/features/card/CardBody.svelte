<!--
  The body of a card: the editor where the reader may edit, the read-only
  view where not.

  The editor is not part of what the app loads first (editor-chunk.svelte.ts).
  Until it has arrived the body is shown with the view, so a card can be read
  at once and never waits for the editor.

  Every change goes to the card store as it happens, and is stored when the
  typing pauses, when the editor is left, and when the card is closed or
  changed for another (body-saver.svelte.ts and CardDialog). When the store
  takes a body the reader did not type (somebody else's, or the other side of
  a conflict) it raises `bodyRevision`, and the editor reads the body again.

  While a conflict waits for the reader's choice the editor holds what they
  wrote and takes no more typing: what would be typed now has no place in
  either of the two texts being chosen between.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';

  import BodyView from '$lib/editor/BodyView.svelte';
  import type { DocNode } from '$lib/editor/schema.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardStore, CardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import BodyConflictNotice from './BodyConflictNotice.svelte';
  import type { BodySaver } from './body-saver.svelte.ts';
  import { editorChunk } from './editor-chunk.svelte.ts';

  interface Props {
    board: BoardStore;
    card: CardStore;
    saver: BodySaver;
  }

  let { board, card, saver }: Props = $props();

  const workspace = $derived(board.workspace);
  const editable = $derived(board.canEdit);
  const Editor = $derived(editable ? editorChunk.component : undefined);
  // Somebody not known (gone, or a mention that names nobody, as an anonymous reader gets them).
  const resolveUser = (userId: string): string =>
    workspace.userName(userId) ?? m['centerPanel.unknown-user']();
  /** Who can be mentioned: the members of the board. */
  const people = $derived(
    board.orderedMembers.map((member) => ({
      id: member.userId,
      name: workspace.userName(member.userId) ?? m['centerPanel.unknown-user'](),
    })),
  );

  $effect(() => {
    if (editable) editorChunk.load();
  });

  /**
   * The reader has put a hand on the body: pressed in it, typed, pasted,
   * dropped. Before that, whatever the editor reports is its own doing (it
   * writes a body in its own way when it is made) and is not an edit: a card
   * that was only looked at is not written.
   */
  let touched = false;
  const HANDS = [
    'pointerdown',
    'keydown',
    'beforeinput',
    'compositionstart',
    'paste',
    'cut',
    'drop',
  ];
  function watchHands(section: HTMLElement): () => void {
    const touch = (): void => {
      touched = true;
    };
    for (const type of HANDS) section.addEventListener(type, touch, { capture: true });
    return () => {
      for (const type of HANDS) section.removeEventListener(type, touch, { capture: true });
    };
  }

  // The editor goes away with the card: what it last reported is sent.
  onDestroy(() => {
    void saver.flush();
  });
</script>

<section
  aria-label={m.card_body()}
  class="flex flex-col gap-3 border-t pt-5"
  data-testid="card-body"
  data-editor={Editor ? 'ready' : editable ? 'loading' : 'none'}
  {@attach watchHands}
>
  {#if card.conflict}
    <BodyConflictNotice conflict={card.conflict} {saver} {resolveUser} />
  {/if}
  {#if Editor}
    <Editor
      doc={card.body}
      revision={card.bodyRevision}
      editable={!card.conflict}
      users={people}
      {resolveUser}
      label={m.card_body()}
      placeholder={m.card_body_placeholder()}
      class="min-h-24 text-base outline-none"
      onChange={(doc: DocNode | null) => {
        if (touched) saver.edited(doc);
      }}
      onBlur={() => void saver.flush()}
      uploadImage={(file: File) => card.uploadImage(file)}
    />
  {:else}
    <BodyView doc={card.body} {resolveUser} class="text-base">
      {#snippet empty()}
        <p class="text-base text-muted-foreground">
          {editable ? m.card_body_placeholder() : m.card_body_empty()}
        </p>
      {/snippet}
      {#snippet invalid()}
        <p class="text-muted-foreground" role="alert">{m.card_body_invalid()}</p>
      {/snippet}
    </BodyView>
    {#if editable && editorChunk.failed}
      <p class="flex flex-wrap items-center gap-2 text-muted-foreground" role="alert">
        {m.card_body_editor_failed()}
        <Button
          variant="outline"
          size="sm"
          class="roomy:min-h-touch"
          onclick={() => {
            editorChunk.load();
          }}
        >
          {m.card_body_editor_retry()}
        </Button>
      </p>
    {/if}
  {/if}
</section>
