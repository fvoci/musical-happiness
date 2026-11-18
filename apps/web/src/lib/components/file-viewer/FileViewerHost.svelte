<!--
  The viewer of a file opened from a card's attachments or a body's file
  block (viewing.svelte.ts), over whatever screen it was opened on, a card's
  dialog included. Mounted once, in the app's frame; the viewer itself
  arrives when it is first needed (chunk.svelte.ts).
-->
<script lang="ts">
  import { viewerChunk } from './chunk.svelte.ts';
  import { viewing } from './viewing.svelte.ts';

  const Viewer = $derived(viewerChunk.component);

  // Gone with the screen it is on (the reader signed out): the file open is let go of, and
  // not shown again over the next.
  $effect(() => () => {
    viewing.forget();
  });
</script>

{#if Viewer && viewing.file}
  <!-- The viewer is made for one file: another is another viewer. -->
  {#key viewing.file}
    <Viewer
      file={viewing.file}
      onClose={() => {
        viewing.close();
      }}
    />
  {/key}
{/if}
