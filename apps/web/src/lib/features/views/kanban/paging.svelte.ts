/**
 * Which column of the board is looked at, and keeping it in view.
 *
 * The board is a row of panels that scrolls sideways: the columns, then
 * what stands after them (`data-panel`). In a pane that fits one column the
 * board comes to rest on a panel (CSS scroll snap) and `current` is the one
 * it rests on. Where several columns fit, the board scrolls freely and
 * `current` only changes when the panel it names has left the view.
 *
 * So a pane that changes between the two (a device unfolded, a window
 * resized, a card opened beside the board) comes back to the column that
 * was looked at: in the middle where one fits, in view where several do.
 */
import { untrack } from 'svelte';

import { nearestRest } from './kanban.ts';

/** How far from the left edge a column is put that is brought into view among several, in pixels. */
const LEFT_MARGIN = 12;

export class Paging {
  /** The element that scrolls. For `bind:this`. */
  scroller = $state<HTMLElement | null>(null);
  /** The panel that is looked at, by its place in the row. */
  current = $state(0);
  readonly #one: () => boolean;

  /**
   * Call while a component is being set up. `one`: the pane fits one column
   * at a time. `count`: how many panels there are.
   */
  constructor(one: () => boolean, count: () => number) {
    this.#one = one;
    $effect(() => {
      const within = this.scroller;
      const snapping = one();
      if (!within) return;
      // On the first frame too, with no animation to wait for.
      untrack(() => {
        const element = this.#panels()[this.current];
        if (!element) return;
        if (snapping) within.scrollLeft = this.#restOf(element, within);
        else if (!this.#wholeInView(element, within)) {
          within.scrollLeft = element.offsetLeft - LEFT_MARGIN;
        }
      });
    });
    // A panel that went away (a column hidden or deleted) leaves the one before it looked at.
    $effect(() => {
      const last = Math.max(0, count() - 1);
      if (untrack(() => this.current) > last) this.current = last;
    });
  }

  #panels(): HTMLElement[] {
    return [...(this.scroller?.querySelectorAll<HTMLElement>(':scope > [data-panel]') ?? [])];
  }

  /** Where the board has to be scrolled to for a panel to sit in the middle. */
  #restOf(element: HTMLElement, within: HTMLElement): number {
    // A panel that is not laid out (a column a pointer drags) rests nowhere.
    if (element.offsetParent === null) return Number.NaN;
    return element.offsetLeft - (within.clientWidth - element.clientWidth) / 2;
  }

  /** Some part of the panel shows. */
  #inView(element: HTMLElement, within: HTMLElement): boolean {
    return (
      element.offsetParent !== null &&
      element.offsetLeft < within.scrollLeft + within.clientWidth &&
      element.offsetLeft + element.offsetWidth > within.scrollLeft
    );
  }

  /** All of the panel shows. */
  #wholeInView(element: HTMLElement, within: HTMLElement): boolean {
    return (
      element.offsetParent !== null &&
      element.offsetLeft >= within.scrollLeft &&
      element.offsetLeft + element.offsetWidth <= within.scrollLeft + within.clientWidth
    );
  }

  /** The board scrolled: by a finger, the wheel, a drag at its edge, or `show`. */
  readonly onScroll = (): void => {
    const within = this.scroller;
    if (!within) return;
    const all = this.#panels();
    if (this.#one()) {
      this.current = nearestRest(
        all.map((element) => this.#restOf(element, within)),
        within.scrollLeft,
      );
      return;
    }
    const looked = all[this.current];
    if (looked && this.#inView(looked, within)) return;
    this.current = Math.max(
      0,
      all.findIndex((element) => this.#inView(element, within)),
    );
  };

  /** Back to the first panel: another view is shown in the same place. */
  reset(): void {
    this.current = 0;
    if (this.scroller) this.scroller.scrollLeft = 0;
  }

  /** Brings a panel to the middle: a tab was pressed. */
  show(at: number): void {
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    this.#panels()[at]?.scrollIntoView({
      behavior: reduced ? 'auto' : 'smooth',
      inline: 'center',
      block: 'nearest',
    });
  }
}
