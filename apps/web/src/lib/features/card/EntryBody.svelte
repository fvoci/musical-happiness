<!--
  The body of the entry of a day of a diary, open on its page: the card's
  kind of body (`CardBody`), in the entry's own room.

  A day without an entry has an editor of its own (the draft): the entry is
  made by what the reader writes first, not before (`EntryStore.write`), and
  the room's editor takes over once the room is there and may be written in,
  with what the draft holds put into it (after what another device wrote
  there meanwhile), the caret where it was. It takes over between syllables,
  never in the middle of one an input method is composing. A draft opened
  again holds what the day kept of it, and is not written in while that is
  being written as the body (`EntryStore.flushing`): the room's editor waits
  for that write, which puts it in the room.

  A day the reader opened by pressing it (`focus`) has the caret put at the
  end of what it holds, once, by whichever editor comes first.
-->
<script lang="ts">
  import type { Editor as TiptapEditor } from '@tiptap/core';

  import type { DocNode } from '$lib/editor/schema.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { EntryStore } from '$lib/state/index.ts';

  import CardBody from './CardBody.svelte';
  import { editorChunk } from './editor-chunk.svelte.ts';

  interface Props {
    entry: EntryStore;
    /** What the draft's editor is called: the day it is of. */
    label: string;
    /** The reader opened the day: the caret goes into it. */
    focus?: boolean;
  }

  let { entry, label, focus = false }: Props = $props();

  /** Who may write, for the body: the reader alone, in their own diary. */
  const host = $derived({ workspace: entry.host.workspace, canEdit: true, orderedMembers: [] });
  const Draft = $derived(editorChunk.component);
  $effect(() => {
    editorChunk.load();
  });

  /** The draft's editor is composing a syllable: the room's editor waits. */
  let composing = $state(false);
  /** Where the caret of the draft was, and whether it had the focus; read as the room's editor comes. */
  let caret = { at: 1, focused: false };

  /** The caret was put in as the day was opened. */
  let placed = false;

  function place(editor: TiptapEditor): void {
    if (!focus || placed) return;
    placed = true;
    editor.commands.focus('end');
  }

  function watchDraft(editor: TiptapEditor): void {
    let going = false;
    const keep = (): void => {
      if (!going) caret = { at: editor.state.selection.from, focused: editor.isFocused };
    };
    for (const event of ['selectionUpdate', 'update', 'focus'] as const) editor.on(event, keep);
    // A draft taken down is left first, then destroyed, at once: that is not the reader leaving it.
    editor.on('blur', () => {
      queueMicrotask(keep);
    });
    editor.on('destroy', () => {
      going = true;
    });
    place(editor);
  }

  /** The room's editor is there: the draft goes into it, and the caret with it. */
  function adopt(editor: TiptapEditor): void {
    const blank = (): void => {
      entry.noteBlank(editor.isEmpty);
    };
    editor.on('update', blank);
    blank();
    const draft = entry.takeDraft();
    if (!draft) {
      place(editor);
      return;
    }
    placed = true;
    const empty = editor.isEmpty;
    if (empty) editor.commands.setContent(draft);
    else editor.commands.insertContentAt(editor.state.doc.content.size, draft.content ?? []);
    blank();
    if (!caret.focused) return;
    // At once, not a frame later as the editor's own command does: the reader is typing.
    editor.commands.setTextSelection(empty ? caret.at : editor.state.doc.content.size);
    editor.view.focus();
  }
</script>

{#snippet draft()}
  {#if Draft}
    <Draft
      doc={entry.draft}
      editable={!entry.flushing}
      {label}
      placeholder={m.diary_entry_placeholder()}
      class="min-h-24 text-base outline-none"
      onChange={(doc: DocNode | null) => {
        entry.write(doc);
      }}
      onComposing={(now: boolean) => {
        composing = now;
      }}
      onEditor={watchDraft}
      uploadFile={(file: File) => entry.uploadFile(file)}
    />
  {/if}
{/snippet}

<CardBody
  container={host}
  card={entry}
  of="entry"
  testId="entry-body"
  waiting={entry.entry && !entry.drafting ? undefined : draft}
  holding={composing || entry.flushing || (entry.drafting && !entry.room?.writable)}
  onEditor={adopt}
/>
{#if entry.unsaved}
  <p class="text-sm text-destructive" role="alert">{m.diary_not_saved({ date: label })}</p>
{/if}
