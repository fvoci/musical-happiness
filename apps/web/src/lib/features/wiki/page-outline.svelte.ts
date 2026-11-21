/**
 * The outline of the page shown: its headings, the one being read, and how
 * it is shown (`mode`), for the page's screen and its parts (`OutlineNav`,
 * `OutlineButton`, `OutlineColumns`).
 *
 * The headings are read from the body as it is drawn (outline-dom.ts), so the
 * read-only view and the editor give the same outline, and what others write
 * comes in with the editor's own drawing. They are read again only when a
 * change of the body touched a heading, at most once a frame: typing in a
 * paragraph costs a look at where the change was and nothing more.
 *
 * The heading being read is the last one whose top has passed a line a
 * quarter of the way down the visible part of the page, found by halving the
 * list (a few measurements a frame, however long the page). A heading the
 * reader chose stays the one marked until they move the page themselves: one
 * near the end of the page cannot reach the line.
 *
 * Beside the page when the page's column has room for both (`side`), unless
 * the reader folded it away, which this device remembers; otherwise behind a
 * button (`button`). None for a page with fewer than two headings.
 */
import { untrack } from 'svelte';

import { fingerPointer } from '$lib/layout/room.svelte.ts';

import {
  focusHeading,
  lastAtOrAbove,
  outlineOf,
  sameOutline,
  scrollToHeading,
  touchesHeadings,
  type OutlineEntry,
} from './outline-dom.ts';

export type { OutlineEntry } from './outline-dom.ts';

/** Where the device remembers that the reader folded the outline away. */
export const OUTLINE_STORAGE_KEY = 'fvoci:wiki-outline';

/** The width the outline takes beside the page, in rem; the page keeps its own. */
const OUTLINE_WIDTH = 12;
/** How far below the head of the page a heading that is gone to stays, in pixels. */
const MARGIN = 16;
/** The events by which the reader moves the page themselves. */
const MOVES = ['wheel', 'touchstart', 'pointerdown', 'keydown'] as const;

export type OutlineMode = 'none' | 'side' | 'button';

function remembered(): boolean {
  try {
    return localStorage.getItem(OUTLINE_STORAGE_KEY) === 'folded';
  } catch {
    return false;
  }
}

const rem = (): number => parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;

export class PageOutline {
  /** The headings with words, in their order. */
  entries = $state.raw<readonly OutlineEntry[]>([]);
  /** The place in `entries` of the heading being read; -1 above the first. */
  current = $state(-1);
  /** The reader folded it away from beside the page. */
  folded = $state(remembered());
  /** The list behind the button is out. */
  open = $state(false);

  /** The width of the page's column, its padding left out; 0 until measured. */
  #width = $state(0);
  /** The height of what shows of the page's column, in pixels. */
  #height = $state(0);
  /** The width the page itself is drawn in, in rem. */
  #page = 48;
  #source: HTMLElement | undefined;
  #scroller: HTMLElement | undefined;
  #bar: HTMLElement | undefined;
  /** The heading the reader chose, marked until they move the page themselves. */
  #chosen: HTMLElement | undefined;
  #stale = false;
  #frame: number | undefined;

  /** `page` is the width the page itself is drawn in, in rem (`max-w-3xl`). */
  constructor(page = 48) {
    this.#page = page;
  }

  /** The width the page is drawn in beside the outline, and the outline's own, in rem. */
  get page(): number {
    return this.#page;
  }
  readonly side = OUTLINE_WIDTH;
  /** How much of the page's column shows, in pixels: the outline beside it is no taller. */
  get height(): number {
    return this.#height;
  }

  /** There is room beside the page for the outline. */
  readonly room = $derived(this.#width > 0 && this.#width >= (this.#page + OUTLINE_WIDTH) * rem());

  readonly mode = $derived<OutlineMode>(
    this.entries.length < 2 ? 'none' : this.room && !this.folded ? 'side' : 'button',
  );

  /** Folds it away from beside the page, or puts it back; remembered on the device. */
  setFolded(folded: boolean): void {
    this.folded = folded;
    try {
      localStorage.setItem(OUTLINE_STORAGE_KEY, folded ? 'folded' : 'beside');
    } catch {
      // Not remembered: it still holds for this visit.
    }
  }

  /** For `{@attach}` on what holds the page's body, and only that: its headings are the outline. */
  readonly source = (element: HTMLElement): (() => void) => {
    this.#source = element;
    this.#stale = true;
    const changes = new MutationObserver((records) => {
      if (touchesHeadings(records)) this.#stale = true;
      if (this.#stale) this.#soon();
    });
    changes.observe(element, { childList: true, characterData: true, subtree: true });
    // A body that grows or shrinks (others' writing above) moves what is being read.
    const sized =
      typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(this.#soon);
    sized?.observe(element);
    this.#soon();
    return () => {
      changes.disconnect();
      sized?.disconnect();
      if (this.#source !== element) return;
      this.#source = undefined;
      this.#chosen = undefined;
      untrack(() => {
        [this.entries, this.current, this.open] = [[], -1, false];
      });
    };
  };

  /** For `{@attach}` on the element the page scrolls in. */
  readonly follow = (element: HTMLElement): (() => void) => {
    this.#scroller = element;
    const measure = (): void => {
      const style = getComputedStyle(element);
      const padding = (parseFloat(style.paddingLeft) || 0) + (parseFloat(style.paddingRight) || 0);
      this.#width = element.clientWidth - padding;
      this.#height = element.clientHeight;
    };
    measure();
    const sized = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(measure);
    sized?.observe(element);
    const moved = (): void => {
      if (!this.#chosen) return;
      this.#chosen = undefined;
      this.#soon();
    };
    for (const name of MOVES)
      element.addEventListener(name, moved, { passive: true, capture: true });
    element.addEventListener('scroll', this.#soon, { passive: true });
    return () => {
      sized?.disconnect();
      for (const name of MOVES) element.removeEventListener(name, moved, { capture: true });
      element.removeEventListener('scroll', this.#soon);
      if (this.#scroller === element) this.#scroller = undefined;
    };
  };

  /** For `{@attach}` on a bar that stays at the head of the page as it scrolls: headings go below it. */
  readonly bar = (element: HTMLElement): (() => void) => {
    this.#bar = element;
    return () => {
      if (this.#bar === element) this.#bar = undefined;
    };
  };

  /**
   * Brings the heading at `at` to the head of the page and the focus to it
   * (outline-dom.ts says where in the editor); marked as the one being read.
   * Gives whether the focus went there.
   */
  go(at: number): boolean {
    const entry = this.entries[at];
    const scroller = this.#scroller;
    if (!entry?.element.isConnected || !scroller) return false;
    this.open = false;
    this.#chosen = entry.element;
    this.current = at;
    // The focus first: WebKit brings the caret into sight as the editor takes the
    // focus, which would cut a smooth scroll short; the scroll then starts from there.
    const focused = focusHeading(entry.element, fingerPointer.current);
    scrollToHeading(scroller, entry.element, this.#head() + MARGIN);
    return focused;
  }

  /** How far the bar at the head of the page reaches down into it, in pixels. */
  #head(): number {
    const [bar, scroller] = [this.#bar, this.#scroller];
    if (!bar?.isConnected || !scroller) return 0;
    return Math.max(0, bar.getBoundingClientRect().bottom - scroller.getBoundingClientRect().top);
  }

  /** Reads what changed at the next frame, once however many changes come before it. */
  readonly #soon = (): void => {
    if (this.#frame !== undefined) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = undefined;
      this.#read();
    });
  };

  #read(): void {
    const source = this.#source;
    if (!source) return;
    if (this.#stale) {
      this.#stale = false;
      const read = outlineOf(source);
      if (!sameOutline(read, this.entries)) this.entries = read;
    }
    const chosen = this.#chosen
      ? this.entries.findIndex((entry) => entry.element === this.#chosen)
      : -1;
    if (chosen < 0) this.#chosen = undefined;
    const current = chosen >= 0 ? chosen : this.#reading();
    if (current !== this.current) this.current = current;
  }

  /** The heading whose top has passed the reading line, by the page as it is now. */
  #reading(): number {
    const scroller = this.#scroller;
    if (!scroller || this.entries.length === 0) return -1;
    const top = scroller.getBoundingClientRect().top + this.#head();
    const line = top + (scroller.clientHeight - this.#head()) / 4;
    return lastAtOrAbove(
      this.entries.length,
      (at) => this.entries[at]?.element.getBoundingClientRect().top ?? Infinity,
      line,
    );
  }
}
