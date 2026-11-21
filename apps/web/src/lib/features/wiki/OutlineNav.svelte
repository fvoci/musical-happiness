<!--
  The outline of a page as a list to choose from: its headings in order, each
  level a step further in than the one above it (the highest level used on
  the page at the edge). The heading being read is marked, by a line at its
  edge and heavier words as well as by its colour, and kept in sight when the
  list is longer than its box (scrolled inside the list, never the page).

  Choosing one brings it to the head of the page and the focus to it
  (`PageOutline.go`); `onChose` hears whether the focus went there.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { ClassValue } from 'svelte/elements';

  import { m } from '$lib/i18n/paraglide/messages.js';

  import type { PageOutline } from './page-outline.svelte.ts';

  interface Props {
    outline: PageOutline;
    id?: string;
    /** What stands above the list: a name, a button to fold it away. */
    head?: Snippet;
    onChose?: (focused: boolean) => void;
    /** Sizes the box; the list scrolls inside what is left of it under `head`. */
    class?: ClassValue;
  }

  let { outline, id, head, onChose, class: className }: Props = $props();

  /** The highest level of the page's headings: at the edge of the list. */
  const top = $derived(Math.min(...outline.entries.map((entry) => entry.level)));

  /** Keeps the item at `at` in sight inside `list`, without moving anything outside it. */
  function inSight(list: HTMLElement, at: number): void {
    const item = list.children[at];
    if (!(item instanceof HTMLElement)) return;
    const above = item.getBoundingClientRect().top - list.getBoundingClientRect().top;
    if (above >= 0 && above + item.offsetHeight <= list.clientHeight) return;
    list.scrollTop += above - (list.clientHeight - item.offsetHeight) / 2;
  }
</script>

<nav
  {id}
  aria-label={m.wiki_outline()}
  class={['flex min-h-0 flex-col', className]}
  data-testid="page-outline"
>
  {@render head?.()}
  <ol
    class="min-h-0 overflow-y-auto overscroll-contain"
    {@attach (list: HTMLElement) => {
      const at = outline.current;
      // Once the list has drawn the change that made it the one being read.
      const frame = requestAnimationFrame(() => {
        inSight(list, at);
      });
      return () => {
        cancelAnimationFrame(frame);
      };
    }}
  >
    {#each outline.entries as entry, at (entry.key)}
      <li>
        <button
          type="button"
          aria-current={at === outline.current ? 'location' : undefined}
          data-level={entry.level}
          class="my-px flex min-h-7 w-full items-center rounded-r-md border-l-2 border-transparent py-1 pr-2 text-left text-sm text-muted-foreground hover:bg-muted hover:text-foreground aria-[current=location]:border-primary aria-[current=location]:font-semibold aria-[current=location]:text-foreground roomy:min-h-touch"
          style:padding-inline-start="{0.5 + (entry.level - top) * 0.75}rem"
          onclick={() => {
            const focused = outline.go(at);
            onChose?.(focused);
          }}
        >
          <span class="line-clamp-2 break-words">{entry.text}</span>
        </button>
      </li>
    {/each}
  </ol>
</nav>
