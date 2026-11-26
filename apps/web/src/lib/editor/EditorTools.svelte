<!--
  The tools around a body being written (BodyEditor.svelte puts them in its
  frame): the bubble over what is chosen (SelectionBubbleBar.svelte), the
  bar over a phone's keyboard (MobileEditorToolbar.svelte), and an image
  opened larger (ImageLightbox.svelte) from the bubble, or from a press on
  an image while the editor is read-only (a reader of a shared body).

  A phone is a screen of touch and of the compact width (app.css
  `--container-medium`): there the bar shows while the editor has the
  focus, and the bubble offers only what it does with a chosen image, since
  the marks are on the bar. Elsewhere the bubble offers the marks over
  chosen text, once a press of the mouse that is choosing it has ended. On
  a touch screen it stands under the text, away from the screen's own menu.
-->
<script lang="ts">
  import type { Editor } from '@tiptap/core';
  import { untrack } from 'svelte';
  import { on } from 'svelte/events';
  import { MediaQuery } from 'svelte/reactivity';

  import { zoomImagesOf, type LightboxImage } from './image-zoom.ts';
  import ImageLightbox from './ImageLightbox.svelte';
  import MobileEditorToolbar from './MobileEditorToolbar.svelte';
  import SelectionBubbleBar from './SelectionBubbleBar.svelte';

  interface Props {
    editor: Editor;
    /** Goes up with every change of the document or the selection. */
    changes: number;
    /** The editor can be written in now. */
    editable: boolean;
    /** The body takes files. */
    uploads: boolean;
  }

  let { editor, changes, editable, uploads }: Props = $props();

  const touch = new MediaQuery('(pointer: coarse)', false);
  // The compact width of app.css (`--container-medium`); a media query cannot read the variable.
  const compact = new MediaQuery('(max-width: 37.5rem)', false);
  const phone = $derived(touch.current && compact.current);

  let focused = $state(false);
  let inBubble = $state(false);
  let inBar = $state(false);
  /** A press of the mouse is choosing text; the bubble waits for its end. */
  let choosing = $state(false);
  let zoomed = $state<LightboxImage>();
  const zoom = (image: LightboxImage): void => {
    zoomed = image;
  };

  /** Follows the focus of the editor and the presses that choose text in it. */
  function followed(current: Editor): () => void {
    // An editor taken out of the page blurs while Svelte takes it out (BodyEditor.svelte `fromEditor`).
    const focus = (): void => {
      untrack(() => (focused = true));
    };
    const blur = (): void => {
      untrack(() => (focused = false));
    };
    current.on('focus', focus);
    current.on('blur', blur);
    const page = current.view.dom.ownerDocument;
    const pressed = on(current.view.dom, 'mousedown', (event) => {
      choosing = event.button === 0;
    });
    const released = on(page, 'mouseup', () => (choosing = false));
    return () => {
      current.off('focus', focus);
      current.off('blur', blur);
      pressed();
      released();
    };
  }

  $effect(() => followed(editor));

  // The editor's own images open on a press while it is read-only (BodyView.svelte does it where a body is only shown).
  $effect(() =>
    zoomImagesOf(editor.view.dom, zoom, {
      buttons: false,
      when: () => !editor.isEditable,
    }),
  );
</script>

<SelectionBubbleBar
  {editor}
  {changes}
  {editable}
  shown={(focused || inBubble) && !choosing}
  marks={!phone}
  below={touch.current}
  bind:within={inBubble}
  onZoom={zoom}
/>
{#if phone && editable && (focused || inBar)}
  <MobileEditorToolbar {editor} {changes} {uploads} bind:within={inBar} />
{/if}
<ImageLightbox image={zoomed} onclose={() => (zoomed = undefined)} />
