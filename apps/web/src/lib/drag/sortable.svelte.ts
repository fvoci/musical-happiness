/**
 * Moving items within and between lists, with a pointer or the keyboard.
 *
 * The page marks its elements and draws; this class watches the input and
 * says what is going on.
 *
 *   data-drag-list="<id>"    the visible box of a list (what scrolls, if it scrolls)
 *   data-drag-item="<id>"    an item, inside its list, focusable
 *   data-drag-scroll="x|y"   a box that scrolls while dragging near its edge:
 *                            the root or anything between it and a list
 *   data-drag-handle         inside an item: only this part starts a drag
 *   data-drag-carried        the stand-in the page draws for the item while
 *                            the keyboard carries it; it receives the focus
 *
 * Nothing is moved in the data until the drop: `active` says which item is
 * on its way and where it would land, and the page draws that (the item
 * hidden where it was, a gap at `active.to`, the item itself at `overlay`).
 * The dragged item must stay in the document, hidden, for as long as it is
 * dragged: a finger's touch events are sent to the element it landed on.
 */
import { tick } from 'svelte';

import {
  clampToRect,
  dropTarget,
  keyboardTarget,
  locateItem,
  nearestList,
  sameTarget,
  type Axis,
  type DropTarget,
  type KeyboardStep,
  type ListOrder,
  type Point,
} from '$lib/logic/drop-target.ts';

import { AutoScroller, type AutoScrollOptions, type ScrollBox } from './auto-scroll.ts';
import {
  pointerDrag,
  type PointerDragHandle,
  type PointerDragOptions,
  type PointerDragSession,
} from './pointer-drag.ts';

const ITEM = 'data-drag-item';
const LIST = 'data-drag-list';
const SCROLL = 'data-drag-scroll';
const HANDLE = 'data-drag-handle';
const CARRIED = 'data-drag-carried';

/** Controls inside an item keep their own presses. */
const CONTROLS =
  'input, textarea, select, button, a[href], [contenteditable]:not([contenteditable="false"])';

/** A drop that changed the place of an item. Indexes count the list without the item. */
export interface SortableMove {
  itemId: string;
  from: DropTarget;
  to: DropTarget;
}

/** The item on its way. */
export interface SortableActive {
  itemId: string;
  mode: 'pointer' | 'keyboard';
  from: DropTarget;
  /** Where it would land if released now. */
  to: DropTarget;
  /** Its size when it was picked up, for the gap and the overlay. */
  width: number;
  height: number;
}

/** A place in words: for the announcements. `position` starts at 1. */
export interface SortablePlace {
  item: string;
  list: string;
  position: number;
  count: number;
}

/** What is read out to a screen reader. */
export interface SortableMessages {
  pickedUp(place: SortablePlace): string;
  moved(place: SortablePlace): string;
  dropped(place: SortablePlace): string;
  cancelled(place: SortablePlace): string;
}

export interface SortableOptions {
  /** The order as data, at the time of the call. */
  order: () => readonly ListOrder[];
  /** Called once per drop that changes the place of the item. */
  onDrop: (move: SortableMove) => void;
  messages: SortableMessages;
  /** Names for the announcements. Default: the ids. */
  itemLabel?: (itemId: string) => string;
  listLabel?: (listId: string) => string;
  /** The direction in which each list stacks its items. Default `y`. */
  axis?: Axis;
  pointer?: Partial<PointerDragOptions>;
  scroll?: Partial<AutoScrollOptions>;
}

export class Sortable {
  #active = $state.raw<SortableActive | undefined>(undefined);
  #overlay = $state.raw<Point | undefined>(undefined);
  #announcement = $state('');

  readonly #options: SortableOptions;
  readonly #scroller: AutoScroller;
  #root: HTMLElement | undefined;
  #pointer: PointerDragHandle | undefined;

  /** Where the pointer is, and where on the item it took hold. */
  #point: Point = { x: 0, y: 0 };
  #grab: Point = { x: 0, y: 0 };
  /** The pointer or something under it moved since the target was last worked out. */
  #stale = false;
  #frame: number | undefined;
  #frameTime = 0;
  /** The item had the focus when the pointer took it, and gets it back. */
  #refocus = false;

  constructor(options: SortableOptions) {
    this.#options = options;
    this.#scroller = new AutoScroller(options.scroll);
  }

  /** The item on its way, or undefined when nothing is dragged. */
  get active(): SortableActive | undefined {
    return this.#active;
  }

  /** The top left corner of the item dragged by a pointer, in the coordinates of the viewport. */
  get overlay(): Point | undefined {
    return this.#overlay;
  }

  /** The latest announcement, to be put in a live region. */
  get announcement(): string {
    return this.#announcement;
  }

  /** Attaches to the element that holds the lists: `{@attach sortable.attach}`. */
  readonly attach = (root: HTMLElement): (() => void) => {
    this.#root = root;
    const pointer = pointerDrag(root, this.#press, this.#options.pointer);
    this.#pointer = pointer;
    root.addEventListener('keydown', this.#onKeyDown);
    return () => {
      this.cancel();
      pointer.destroy();
      root.removeEventListener('keydown', this.#onKeyDown);
      this.#pointer = undefined;
      this.#root = undefined;
    };
  };

  /** Ends whatever is going on and puts everything back. */
  cancel(): void {
    this.#pointer?.cancel();
    this.#finish(false);
  }

  // ------------------------------------------------------------- pointer

  readonly #press = (event: PointerEvent): PointerDragSession | undefined => {
    if (this.#active) return undefined;
    const item = this.#itemAt(event.target);
    const itemId = item?.getAttribute(ITEM);
    if (!item || !itemId) return undefined;
    const from = locateItem(this.#options.order(), itemId);
    if (!from) return undefined;

    const pressed = item.getBoundingClientRect();
    const grab = { x: event.clientX - pressed.left, y: event.clientY - pressed.top };
    return {
      start: (point) => {
        const { width, height } = item.getBoundingClientRect();
        this.#refocus = document.activeElement === item;
        this.#grab = grab;
        this.#point = point;
        this.#active = { itemId, mode: 'pointer', from, to: from, width, height };
        this.#overlay = { x: point.x - grab.x, y: point.y - grab.y };
        this.#root?.addEventListener('scroll', this.#onScroll, { capture: true, passive: true });
        this.#stale = true;
        this.#frameTime = performance.now();
        this.#frame = requestAnimationFrame(this.#onFrame);
      },
      move: (point) => {
        this.#point = point;
        this.#overlay = { x: point.x - this.#grab.x, y: point.y - this.#grab.y };
        this.#stale = true;
      },
      drop: (point) => {
        this.#point = point;
        this.#retarget();
        this.#finish(true);
      },
      cancel: () => {
        this.#scroller.restore();
        this.#finish(false);
      },
    };
  };

  /** The item a press on `target` would drag, if any. */
  #itemAt(target: EventTarget | null): HTMLElement | undefined {
    if (!(target instanceof Element)) return undefined;
    const item = target.closest<HTMLElement>(`[${ITEM}]`);
    if (!item || !this.#root?.contains(item)) return undefined;
    const control = target.closest(CONTROLS);
    if (control && control !== item && item.contains(control)) return undefined;
    if (item.querySelector(`[${HANDLE}]`) && !target.closest(`[${HANDLE}]`)) return undefined;
    return item;
  }

  readonly #onScroll = (): void => {
    this.#stale = true;
  };

  /**
   * Once per frame while a pointer drags: the target follows the pointer,
   * the boxes scroll, and the target follows what scrolled under the pointer.
   */
  readonly #onFrame = (now: number): void => {
    if (this.#active?.mode !== 'pointer') return;
    // A frame that took long (a hidden tab, a slow layout) must not turn into a jump.
    const elapsed = Math.min(now - this.#frameTime, 50);
    this.#frameTime = now;
    if (this.#stale) this.#retarget();
    if (this.#scroller.step(this.#point, this.#scrollBoxes(), elapsed)) this.#retarget();
    this.#frame = requestAnimationFrame(this.#onFrame);
  };

  /** Works out the target from where the pointer and the items are now. */
  #retarget(): void {
    const root = this.#root;
    const active = this.#active;
    if (!root || !active) return;
    this.#stale = false;

    // The boxes of the lists first, and then the items of the one list the
    // pointer picks: the cost of a frame does not grow with the other lists.
    const bounds = root.getBoundingClientRect();
    const boxes = [...root.querySelectorAll<HTMLElement>(`[${LIST}]`)].map((element) => ({
      element,
      id: element.getAttribute(LIST) ?? '',
      rect: element.getBoundingClientRect(),
    }));
    const hit = nearestList(clampToRect(this.#point, bounds), boxes, this.#options.axis);
    if (!hit) return;
    const items = [...hit.element.querySelectorAll<HTMLElement>(`[${ITEM}]`)]
      .filter((element) => element.getAttribute(ITEM) !== active.itemId)
      .map((element) => element.getBoundingClientRect());
    const to = dropTarget(this.#point, [{ id: hit.id, rect: hit.rect, items }], {
      axis: this.#options.axis,
      bounds,
    });
    if (to && !sameTarget(to, active.to)) this.#active = { ...active, to };
  }

  /** The boxes that may scroll now: those around the list the item would land in. */
  #scrollBoxes(): ScrollBox[] {
    const root = this.#root;
    const active = this.#active;
    if (!root || !active) return [];
    const boxes: ScrollBox[] = [];
    let element = root.querySelector<HTMLElement>(`[${LIST}="${CSS.escape(active.to.listId)}"]`);
    while (element && root.contains(element)) {
      const axis = element.getAttribute(SCROLL);
      if (axis === 'x' || axis === 'y') boxes.push({ element, axis });
      element = element.parentElement;
    }
    return boxes;
  }

  // ------------------------------------------------------------ keyboard

  /** Space on a focused item picks it up. */
  readonly #onKeyDown = (event: KeyboardEvent): void => {
    if (this.#active || event.key !== ' ' || event.repeat) return;
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const item = event.target;
    if (!(item instanceof HTMLElement) || !item.hasAttribute(ITEM)) return;
    const itemId = item.getAttribute(ITEM) ?? '';
    const from = locateItem(this.#options.order(), itemId);
    if (!from) return;

    event.preventDefault();
    const { width, height } = item.getBoundingClientRect();
    this.#active = { itemId, mode: 'keyboard', from, to: from, width, height };
    // On the window and capturing: the focus sits on the stand-in, which the
    // page may draw anywhere, and no other handler may act on these keys.
    window.addEventListener('keydown', this.#onCarryKey, true);
    window.addEventListener('pointerdown', this.#onCarryPointer, true);
    this.#announce('pickedUp', itemId, from);
    void this.#focusCarried();
  };

  readonly #onCarryKey = (event: KeyboardEvent): void => {
    const active = this.#active;
    if (active?.mode !== 'keyboard') return;
    const step = this.#stepFor(event.key);
    if (step) {
      const to = keyboardTarget(active.to, step, this.#options.order(), active.itemId);
      if (!sameTarget(to, active.to)) {
        this.#active = { ...active, to };
        this.#announce('moved', active.itemId, to);
        void this.#focusCarried();
      }
    } else if (event.key === ' ' || event.key === 'Enter') {
      this.#finish(true);
    } else if (event.key === 'Escape') {
      this.#finish(false);
    } else if (event.key !== 'Tab') {
      return;
    }
    // Tab is swallowed too: the focus stays on the item until it is put down.
    event.preventDefault();
    event.stopPropagation();
  };

  /** Reaching for the pointer while carrying gives the carrying up. */
  readonly #onCarryPointer = (): void => {
    if (this.#active?.mode === 'keyboard') this.#finish(false);
  };

  /** The step a key stands for: along the axis within a list, across it between lists. */
  #stepFor(key: string): KeyboardStep | undefined {
    const root = this.#root;
    const rtl = root !== undefined && getComputedStyle(root).direction === 'rtl';
    const vertical = (this.#options.axis ?? 'y') === 'y';
    switch (key) {
      case 'ArrowUp':
        return vertical ? 'previous' : 'previousList';
      case 'ArrowDown':
        return vertical ? 'next' : 'nextList';
      case 'ArrowLeft':
      case 'ArrowRight': {
        const forward = (key === 'ArrowRight') !== rtl;
        if (vertical) return forward ? 'nextList' : 'previousList';
        return forward ? 'next' : 'previous';
      }
      case 'Home':
        return 'first';
      case 'End':
        return 'last';
      default:
        return undefined;
    }
  }

  /** Moves the focus to the stand-in once the page has drawn it at its new place. */
  async #focusCarried(): Promise<void> {
    await tick();
    const carried = this.#root?.querySelector<HTMLElement>(`[${CARRIED}]`);
    if (!carried) return;
    if (document.activeElement !== carried) carried.focus();
    carried.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  // ---------------------------------------------------------------- both

  #finish(commit: boolean): void {
    const active = this.#active;
    if (!active) return;

    if (this.#frame !== undefined) cancelAnimationFrame(this.#frame);
    this.#frame = undefined;
    this.#root?.removeEventListener('scroll', this.#onScroll, { capture: true });
    this.#scroller.clear();
    window.removeEventListener('keydown', this.#onCarryKey, true);
    window.removeEventListener('pointerdown', this.#onCarryPointer, true);

    const end = commit ? active.to : active.from;
    this.#announce(commit ? 'dropped' : 'cancelled', active.itemId, end);
    this.#active = undefined;
    this.#overlay = undefined;
    if (commit && !sameTarget(active.from, active.to)) {
      this.#options.onDrop({ itemId: active.itemId, from: active.from, to: active.to });
    }
    if (active.mode === 'keyboard') void this.#focusItem(active.itemId, false);
    else if (this.#refocus) void this.#focusItem(active.itemId, true);
  }

  async #focusItem(itemId: string, preventScroll: boolean): Promise<void> {
    await tick();
    this.#root
      ?.querySelector<HTMLElement>(`[${ITEM}="${CSS.escape(itemId)}"]`)
      ?.focus({ preventScroll });
  }

  #announce(kind: keyof SortableMessages, itemId: string, at: DropTarget): void {
    const list = this.#options.order().find((candidate) => candidate.id === at.listId);
    const others = list ? list.items.filter((id) => id !== itemId).length : 0;
    this.#announcement = this.#options.messages[kind]({
      item: this.#options.itemLabel?.(itemId) ?? itemId,
      list: this.#options.listLabel?.(at.listId) ?? at.listId,
      position: at.index + 1,
      count: others + 1,
    });
  }
}
