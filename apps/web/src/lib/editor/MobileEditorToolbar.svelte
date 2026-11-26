<!--
  The bar over a phone's keyboard while a body is written in: one row that
  scrolls sideways, of what is done to the line (format-actions.ts: three
  headings, a to-do list, two lists, a quote, a table, a file) and to the
  text (the marks), with undo and redo at its end, always in reach. Each
  button is as tall and as wide as a finger needs (44px).

  It is fixed to the bottom of the frame the body is in (the window, or a
  card's dialog, which is moved and so holds what is fixed in it), lifted
  to the bottom of what can be seen of the box that scrolls the body: above
  the keyboard (measured, since a card's frame is already shrunk by
  `--keyboard` and a page is not), and above what is pinned under that box
  (a card's comment field). Fixed, it takes
  no room: the page does not move when it comes or goes, so a press below
  the body that takes the focus away lands where it was aimed. The editor
  keeps the caret clear of it (EditorTools.svelte). A press on it keeps the
  focus, and so the keyboard, in the editor.
-->
<script lang="ts">
  import type { Editor } from '@tiptap/core';
  import { on } from 'svelte/events';

  import { m } from '$lib/i18n/paraglide/messages.js';

  import { focusedFormat, keepEditorFocus, moveAlongBar, tabStop } from './bar-keys.ts';
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

  /** The button focused last; Tab comes back to it, the arrows go to the rest. */
  let last = $state<string>();
  const stop = $derived(
    tabStop(
      [...states.line, ...states.history].map((state) => ({ id: state.action.id, can: state.can })),
      last,
    ),
  );

  /** Zoomed in, the part of the page seen is smaller for another reason than a keyboard. */
  const ZOOMED = 1.01;

  /** Lifts the bar to the bottom of what can be seen of its scrolling box. */
  function lifted(bar: HTMLElement): (() => void) | undefined {
    const viewport = bar.ownerDocument.defaultView?.visualViewport;
    if (!viewport) return undefined;
    const box = scrollingBox(bar);
    const lift = (): void => {
      // At the frame's bottom first, to read where that bottom is.
      bar.style.bottom = '0px';
      const frame = bar.getBoundingClientRect().bottom;
      const seen = viewport.scale > ZOOMED ? frame : viewport.offsetTop + viewport.height;
      const floor = Math.min(box?.getBoundingClientRect().bottom ?? frame, seen);
      bar.style.bottom = `${String(Math.max(0, Math.round(frame - floor)))}px`;
    };
    lift();
    const resized = on(viewport, 'resize', lift);
    const scrolled = on(viewport, 'scroll', lift);
    // What is pinned under the box (a comment being written) may grow.
    const sized = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(lift);
    if (box) sized?.observe(box);
    return () => {
      resized();
      scrolled();
      sized?.disconnect();
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
    tabindex={state.action.id === stop ? 0 : -1}
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
  data-editor-tool="bar"
  {@attach lifted}
  onfocusin={(event) => {
    within = true;
    last = focusedFormat(event) ?? last;
  }}
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
    position: fixed;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 40;
    display: flex;
    align-items: stretch;
    padding-right: var(--safe-right, 0px);
    padding-left: var(--safe-left, 0px);
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
    /* Its end fades: there is more to scroll to. Only the alpha of the mask counts. */
    mask-image: linear-gradient(to right, black calc(100% - 1.5rem), transparent);
  }
  .doc-keyboard-bar-line::-webkit-scrollbar {
    display: none;
  }
  /* Room after the last button, so that it scrolls clear of the fade. */
  .doc-keyboard-bar-line::after {
    flex: none;
    width: 1.5rem;
    content: '';
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
