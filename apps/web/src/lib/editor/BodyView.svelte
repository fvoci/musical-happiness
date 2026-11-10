<!--
  A document body for reading: a card preview, a comment, a shared board.

  A thin shell around `renderBody` (render.ts), which does the drawing without
  the editor. Everything comes in as values and callbacks. A body that is not
  a valid document is not drawn at all; `invalid` says what to show in its
  place, and `empty` what to show for a body with nothing in it.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { ClassValue } from 'svelte/elements';

  import './editor.css';
  import { renderBody } from './render.ts';

  interface Props {
    /** The stored document, parsed from JSON. `null` or nothing is an empty body. */
    doc: unknown;
    /** The name of a mentioned user; the identifier is shown when there is none. */
    resolveUser?: (userId: string) => string | undefined;
    /** The accessible name of the checkbox of a task, from the text of the task. */
    taskLabel?: (text: string, checked: boolean) => string;
    /** Shown in place of a body that cannot be shown; gets what is wrong with it. */
    invalid?: Snippet<[problems: string[]]>;
    /** Shown in place of an empty body. */
    empty?: Snippet;
    class?: ClassValue;
  }

  let { doc, resolveUser, taskLabel, invalid, empty, class: className }: Props = $props();

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
    }}
  ></div>
{/if}
