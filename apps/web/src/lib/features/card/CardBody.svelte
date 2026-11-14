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

  import { FORMULA_DROPPED, IMAGE_DROPPED, LATE_NOTE, lostNote, roomNote } from './body-notes.ts';
  import { bodyReferences } from './body-references.ts';
  import { keepCaret } from './caret-keeper.ts';
  import { editorChunk } from './editor-chunk.svelte.ts';

  interface Props {
    board: BoardStore;
    card: CardStore;
  }

  let { board, card }: Props = $props();

  const references = $derived(bodyReferences(board));

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

  /** The workspace the room is asked for in; the room opens once it is known. */
  const workspaceId = $derived(workspace.info?.id);

  // The room is the card's while this is shown; going away leaves it. Only
  // another card (or the workspace becoming known) opens another room:
  // nothing the opening reads does.
  $effect(() => {
    const store = card;
    if (workspaceId === undefined) return;
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
  /** How many times the editor was made anew for the same document. */
  let made = $state(0);

  /** Joining and receiving are usually over at once: their line is put in only when they last. */
  let lateShown = $state(false);
  const late = $derived(note?.late ?? false);
  /** The line said under the body: none while the editor's code failed (that says so itself). */
  const shownNote = $derived(
    !(room && editorChunk.failed) && note && (!note.late || lateShown) ? note : undefined,
  );
  $effect(() => {
    if (!late) return;
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
  {#if card.lost.length > 0}
    <div
      class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 bg-destructive/10 p-3"
      role="alert"
      data-testid="body-lost"
    >
      <div class="min-w-0 flex-1">
        {#each card.lost as why (why)}
          <p>{lostNote(why)}</p>
        {/each}
      </div>
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
    <!-- Made anew for another document, and when one it could not draw fits again. -->
    {#key made}
      {#key doc}
        <Editor
          doc={card.body}
          collab={doc}
          editable={writable}
          canWrite={board.canEdit && room?.mode === 'write'}
          onFitsAgain={() => {
            made += 1;
          }}
          onImageDropped={() => {
            card.sayLost(IMAGE_DROPPED);
          }}
          onDraftDropped={() => {
            card.sayLost(FORMULA_DROPPED);
          }}
          users={people}
          {resolveUser}
          {references}
          label={m.card_body()}
          placeholder={writable ? m.card_body_placeholder() : ''}
          class="min-h-24 text-base outline-none"
          uploadImage={(file: File) => card.uploadImage(file)}
          onEditor={(editor: TiptapEditor) => {
            caret.take(editor, writable);
          }}
        />
      {/key}
    {/key}
  {:else}
    <div {@attach caret.watch}>
      <BodyView doc={card.body} {resolveUser} {references} class="text-base">
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
  <!-- The line is in a live region that is always there, so that it is said aloud; its button is not. -->
  <div class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
    <p role="status">
      {#if shownNote}
        <span data-testid="body-room-note" data-reason={room?.reason}>{shownNote.text}</span>
      {/if}
    </p>
    {#if shownNote?.retry}
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
  </div>
</section>
