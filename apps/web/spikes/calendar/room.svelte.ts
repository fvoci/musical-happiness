/**
 * The box the calendar itself is drawn in: the pane less whatever stands
 * beside the grid. The grid follows the width of this box, not of the pane
 * and not of the window, in the way the parts of the mobile mock-ups follow
 * theirs (`../mobile/layout.svelte.ts`). Measured whenever it changes.
 *
 * The width at which it changes is not a new one: bars need the room two
 * columns of a kanban need (`--container-columns`, 34rem, 544 pixels; a cell
 * is then 77 pixels wide, four or five Korean characters of a title).
 */
const root = document.documentElement;

/** A width from the style sheet, in pixels at the reader's text size. */
function boundary(name: string): number {
  const value = getComputedStyle(root).getPropertyValue(name).trim();
  if (!value.endsWith('rem')) throw new Error(`the style sheet does not give ${name} in rem`);
  return parseFloat(value) * parseFloat(getComputedStyle(root).fontSize);
}

class CalendarRoom {
  /** 0 until the box is on the page. */
  width = $state(0);

  readonly measured = $derived(this.width > 0);
  /** There is room for a grid whose cells hold bars with titles. */
  readonly bars = $derived(this.width >= boundary('--container-columns'));

  /** Follows an element from now on. For `{@attach}`. */
  readonly attach = (element: HTMLElement): (() => void) => {
    this.width = element.clientWidth;
    const observer = new ResizeObserver((entries) => {
      const entry = entries.at(-1);
      if (entry) this.width = entry.contentRect.width;
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  };
}

export const room = new CalendarRoom();
