<!--
  The body of a card, edited together with whoever else has it open.

  The body is opened in its room (`CardStore.openBody`) and left when this
  goes away. The editor is bound to the room's document: what the reader
  types goes to the others as it is typed, and theirs comes in. The document
  is the server's; until it has arrived (and the editor, which is not part
  of what the app loads first: editor-chunk.svelte.ts) the body is shown as
  the card was read, with the read-only view, so that a card can be read at
  once. Where there is no room (a reader by a public link) that view stays.

  The editor is read-only, with a line that says why, while the reader may
  only read the board, while the connection is away (it comes back by
  itself), while the document is being sent again, and when the room cannot
  be joined. When changes of the reader were dropped with their document,
  a notice says so until it is dismissed. Another document (the server made
  it anew) is another editor: the caret is put back where it was.
-->
<script lang="ts">
  import type { Editor as TiptapEditor } from '@tiptap/core';
  import { untrack } from 'svelte';

  import BodyView from '$lib/editor/BodyView.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardStore, CardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { LATE_NOTE, lostNote, roomNote } from './body-notes.ts';
  import { keepCaret } from './caret-keeper.ts';
  import { editorChunk } from './editor-chunk.svelte.ts';

  interface Props {
    board: BoardStore;
    card: CardStore;
  }

  let { board, card }: Props = $props();

  const workspace = $derived(board.workspace);
  const room = $derived(card.room);
  const doc = $derived(room?.doc);
  const Editor = $derived(room ? editorChunk.component : undefined);
  /** The reader may write: the board lets them, and the room does now. */
  const writable = $derived(board.canEdit && (room?.writable ?? false));
  const note = $derived(room ? roomNote(room, board.canEdit) : undefined);
  /** The room is being joined, or its document sent again: what is shown is about to change. */
  const settling = $derived(room?.status === 'joining' || room?.status === 'receiving');

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

  // The room is the card's while this is shown; going away leaves it. Only
  // another card opens another room: nothing the opening reads does.
  $effect(() => {
    const store = card;
    untrack(() => {
      store.openBody();
    });
    return () => {
      store.closeBody();
    };
  });

  $effect(() => {
    if (room) editorChunk.load();
  });

  const caret = keepCaret();

  /** Joining and receiving are usually over at once: their line is put in only when they last. */
  let lateShown = $state(false);
  $effect(() => {
    if (!note?.late) return;
    const timer = setTimeout(() => {
      lateShown = true;
    }, LATE_NOTE);
    return () => {
      clearTimeout(timer);
      lateShown = false;
    };
  });
</script>

<section
  aria-label={m.card_body()}
  class="flex flex-col gap-3 border-t pt-5"
  data-testid="card-body"
  data-editor={Editor && doc ? (writable ? 'ready' : 'reading') : room ? 'loading' : 'none'}
  data-room={room?.status ?? 'none'}
  data-settling={settling ? '' : undefined}
>
  {#if card.lost}
    <div
      class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 bg-destructive/10 p-3"
      role="alert"
      data-testid="body-lost"
    >
      <p class="min-w-0 flex-1">{lostNote(card.lost)}</p>
      <Button
        variant="outline"
        size="sm"
        class="roomy:min-h-touch"
        onclick={() => {
          card.dismissLost();
        }}
      >
        {m.ui_close()}
      </Button>
    </div>
  {/if}
  {#if Editor && doc}
    {#key doc}
      <Editor
        doc={card.body}
        collab={doc}
        editable={writable}
        users={people}
        {resolveUser}
        label={m.card_body()}
        placeholder={writable ? m.card_body_placeholder() : ''}
        class="min-h-24 text-base outline-none"
        uploadImage={(file: File) => card.uploadImage(file)}
        onEditor={(editor: TiptapEditor) => {
          caret.take(editor, writable);
        }}
      />
    {/key}
  {:else}
    <div {@attach caret.watch}>
      <BodyView doc={card.body} {resolveUser} class="text-base">
        {#snippet empty()}
          <p class="text-base text-muted-foreground">
            {board.canEdit ? m.card_body_placeholder() : m.card_body_empty()}
          </p>
        {/snippet}
        {#snippet invalid()}
          <p class="text-muted-foreground" role="alert">{m.card_body_invalid()}</p>
        {/snippet}
      </BodyView>
    </div>
  {/if}
  {#if room && editorChunk.failed}
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
  <!-- A live region that is always there: a line put into it is said aloud. -->
  <div role="status">
    {#if note && (!note.late || lateShown)}
      <p
        class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground"
        data-testid="body-room-note"
        data-reason={room?.reason}
      >
        {note.text}
        {#if note.retry}
          <Button
            variant="outline"
            size="sm"
            class="roomy:min-h-touch"
            onclick={() => {
              card.retryBody();
            }}
          >
            {m.card_body_editor_retry()}
          </Button>
        {/if}
      </p>
    {/if}
  </div>
</section>
