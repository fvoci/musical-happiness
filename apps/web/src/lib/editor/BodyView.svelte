<!--
  A document body for reading: a card preview, a comment, a shared board.

  A thin shell around `renderBody` (render.ts), which does the drawing without
  the editor. Everything comes in as values and callbacks. A body that is not
  a valid document is not drawn at all; `invalid` says what to show in its
  place, and `empty` what to show for a body with nothing in it.

  `renderBody` only makes room for formulas and diagrams; once the body is in
  the page, `drawEmbeds` (embeds.ts) draws them. The libraries for that are
  fetched only when a body has a formula or a diagram. So are the bar, the
  colours and the line numbers of a code block (code-later.ts).

  A reference is an element that holds an identifier; what it names is drawn
  into it from `references` (reference-chip.ts), and again whenever what the
  caller knows of it changes. Without `references` each shows as what the
  reader may not see. A file block is a place too, and its file's name, size
  and link are drawn into it from the file source (file-view.ts).
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { ClassValue } from 'svelte/elements';

  import './editor.css';
  import { CODE_COPY_TEXTS } from './code-copy-texts.ts';
  import { drawCodeBlocksLater } from './code-later.ts';
  import { drawEmbeds } from './embeds.ts';
  import { drawFileBlocks } from './file-view.ts';
  import { attachReferences, type ReferenceDrawing } from './reference-chip.ts';
  import { renderBody } from './render.ts';

  interface Props {
    /** The stored document, parsed from JSON. `null` or nothing is an empty body. */
    doc: unknown;
    /** The name of a mentioned user; the identifier is shown when there is none. */
    resolveUser?: (userId: string) => string | undefined;
    /** The accessible name of the checkbox of a task, from the text of the task. */
    taskLabel?: (text: string, checked: boolean) => string;
    /** What the references of the body are drawn from, and how one is opened. */
    references?: ReferenceDrawing;
    /** Shown in place of a body that cannot be shown; gets what is wrong with it. */
    invalid?: Snippet<[problems: string[]]>;
    /** Shown in place of an empty body. */
    empty?: Snippet;
    class?: ClassValue;
  }

  let {
    doc,
    resolveUser,
    taskLabel,
    references,
    invalid,
    empty,
    class: className,
  }: Props = $props();

  const body = $derived.by(() => {
    const rendered = renderBody(doc, { resolveUser, taskLabel });
    // A fragment gives its nodes away once; the list can be placed again.
    return rendered.ok ? { ...rendered, nodes: [...rendered.fragment.childNodes] } : rendered;
  });
</script>

{#if !body.ok}
  {@render invalid?.(body.problems)}
{:else if body.empty}
  {@render empty?.()}
{:else}
  {@const nodes = body.nodes}
  <div
    class={['doc-body', className]}
    {@attach (element: HTMLElement) => {
      element.replaceChildren(...nodes);
      // Each file block's name, size and link, drawn as in the editor (file-view.ts).
      drawFileBlocks(element);
      // Gives back what stops the drawing when the body changes or goes away.
      return drawEmbeds(element);
    }}
    {@attach (element: HTMLElement) => attachReferences(element, references, nodes)}
    {@attach (element: HTMLElement) => drawCodeBlocksLater(element, CODE_COPY_TEXTS, nodes)}
  ></div>
{/if}
