<!--
  The bar over a phone's keyboard while a body is written in: one row that
  scrolls sideways, of what is done to the line (format-actions.ts: three
  headings, a to-do list, two lists, a quote, a table, a file) and to the
  text (the marks), with undo and redo at its end, always in reach. Each
  button is as tall and as wide as a finger needs (44px).

  It stands at the bottom of what can be seen of the box that scrolls the
  body: above the keyboard where the keyboard covers that box (the covered
  part is measured, since a card's frame is already shrunk by `--keyboard`
  and a page is not), right under the text where the body ends before. A
  press keeps the focus, and so the keyboard, in the editor.
-->
<script lang="ts">
  import type { Editor } from '@tiptap/core';
  import { on } from 'svelte/events';

  import { m } from '$lib/i18n/paraglide/messages.js';

  import { keepEditorFocus, moveAlongBar } from './bar-keys.ts';
  import { scrollingBox } from './bubble-place.ts';
  import {
    HISTORY_ACTIONS,
    MARK_ACTIONS,
    lineActions,
    type FormatAction,
  } from './format-actions.ts';

  interface Props {
    editor: Editor;
    /** Goes up with every change of the document or the selection. */
    changes: number;
    /** The body takes files. */
    uploads: boolean;
    /** The focus is in the bar. Bindable. */
    within?: boolean;
  }

  let { editor, changes, uploads, within = $bindable(false) }: Props = $props();

  const line = $derived([...lineActions(uploads), ...MARK_ACTIONS]);

  /** Each button's state where the selection is now; asked again on each change. */
  function statesNow(..._changed: unknown[]) {
    const of = (action: FormatAction) => ({
      action,
      can: action.can(editor),
      on: action.active?.(editor) ?? false,
    });
    return { line: line.map(of), history: HISTORY_ACTIONS.map(of) };
  }
  const states = $derived(statesNow(changes, line));

  /** Zoomed in, the part of the page seen is smaller for another reason than a keyboard. */
  const ZOOMED = 1.01;

  /** Lifts the bar by the part of its scrolling box that the keyboard covers. */
  function lifted(bar: HTMLElement): (() => void) | undefined {
    const viewport = bar.ownerDocument.defaultView?.visualViewport;
    if (!viewport) return undefined;
    const lift = (): void => {
      const box = scrollingBox(bar);
      const bottom = box ? box.getBoundingClientRect().bottom : window.innerHeight;
      const seen = viewport.offsetTop + viewport.height;
      const covered = viewport.scale > ZOOMED ? 0 : Math.max(0, Math.round(bottom - seen));
      bar.style.bottom = `${String(covered)}px`;
    };
    lift();
    const resized = on(viewport, 'resize', lift);
    const scrolled = on(viewport, 'scroll', lift);
    return () => {
      resized();
      scrolled();
    };
  }
</script>

{#snippet button(state: { action: FormatAction; can: boolean; on: boolean })}
  <button
    type="button"
    data-format={state.action.id}
    aria-label={state.action.label()}
    aria-pressed={state.action.active ? state.on : undefined}
    disabled={!state.can}
    onmousedown={keepEditorFocus}
    onclick={() => {
      state.action.run(editor);
    }}
  >
    <state.action.icon class="doc-keyboard-bar-icon" aria-hidden={true} />
  </button>
{/snippet}

<div
  class="doc-keyboard-bar"
  role="toolbar"
  tabindex="-1"
  aria-label={m.editor_format_bar()}
  data-testid="mobile-editor-toolbar"
  {@attach lifted}
  onfocusin={() => (within = true)}
  onfocusout={(event) => {
    const next = event.relatedTarget;
    if (!(next instanceof Node && event.currentTarget.contains(next))) within = false;
  }}
  onkeydown={(event) => {
    moveAlongBar(event, event.currentTarget);
  }}
>
  <div class="doc-keyboard-bar-line">
    {#each states.line as state (state.action.id)}
      {@render button(state)}
    {/each}
  </div>
  <div class="doc-keyboard-bar-history">
    {#each states.history as state (state.action.id)}
      {@render button(state)}
    {/each}
  </div>
</div>

<style>
  .doc-keyboard-bar {
    position: sticky;
    bottom: 0;
    z-index: 30;
    display: flex;
    align-items: stretch;
    margin-top: 0.5rem;
    border-top: 1px solid var(--border);
    background: var(--doc-surface, var(--background));
    color: var(--foreground);
  }
  .doc-keyboard-bar:focus {
    outline: none;
  }
  .doc-keyboard-bar-line {
    display: flex;
    flex: 1 1 auto;
    min-width: 0;
    overflow-x: auto;
    overscroll-behavior-x: contain;
    scrollbar-width: none;
  }
  .doc-keyboard-bar-line::-webkit-scrollbar {
    display: none;
  }
  .doc-keyboard-bar-history {
    display: flex;
    flex: none;
    border-left: 1px solid var(--border);
  }
  .doc-keyboard-bar button {
    display: grid;
    flex: none;
    place-items: center;
    width: 2.75rem;
    height: 2.75rem;
    border: 0;
    background: transparent;
    color: inherit;
    touch-action: manipulation;
  }
  .doc-keyboard-bar button[aria-pressed='true'] {
    background: color-mix(in oklab, var(--ring) 18%, transparent);
  }
  .doc-keyboard-bar button:disabled {
    opacity: 0.35;
  }
  .doc-keyboard-bar button:focus-visible {
    outline: 2px solid var(--ring);
    outline-offset: -2px;
  }
  .doc-keyboard-bar :global(.doc-keyboard-bar-icon) {
    width: 1.25rem;
    height: 1.25rem;
  }
</style>
