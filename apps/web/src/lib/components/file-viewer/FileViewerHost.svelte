<!--
  The viewer of a file opened from a card's attachments or a body's file
  block (viewing.svelte.ts), over whatever screen it was opened on, a card's
  dialog included. Mounted once, in the app's frame; the viewer itself
  arrives when it is first needed (chunk.svelte.ts).

  While a version of the file is in the editor (FileEditor.svelte), the
  editor is shown in the viewer's place; the viewer comes back, made anew,
  when the editor is closed.
-->
<script lang="ts">
  import type { EditedFile } from '$lib/file-viewer/editing.svelte.ts';

  import { editorChunk, viewerChunk } from './chunk.svelte.ts';
  import { viewing } from './viewing.svelte.ts';

  const Viewer = $derived(viewerChunk.component);
  const Editor = $derived(editorChunk.component);

  // Gone with the screen it is on (the reader signed out): the file open is let go of, and
  // not shown again over the next.
  $effect(() => () => {
    viewing.forget();
  });
</script>

{#if Editor && viewing.edited && viewing.versions}
  <Editor
    file={viewing.edited}
    versions={viewing.versions}
    onClose={() => {
      viewing.endEdit();
    }}
  />
{:else if Viewer && viewing.file}
  <!-- The viewer is made for one file: another is another viewer. -->
  {#key viewing.file}
    <Viewer
      file={viewing.file}
      versions={viewing.versions}
      onClose={() => {
        viewing.close();
      }}
      onEdit={(edited: EditedFile) => {
        void viewing.edit(edited);
      }}
      returnFocus={() => {
        viewing.returnFocus();
      }}
    />
  {/key}
{/if}
