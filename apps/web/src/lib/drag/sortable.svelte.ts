/**
 * Moving items within and between lists, with a pointer or the keyboard.
 *
 * The page marks its elements and draws; this class watches the input and
 * says what is going on.
 *
 *   data-drag-list="<id>"    the visible box of a list (what scrolls, if it
 *                            scrolls). The root itself may be one
 *   data-drag-item="<id>"    an item, inside its list
 *   data-drag-zone="<id>"    a target that is no list: a drop on it has no
 *                            place, only the zone
 *   data-drag-scroll="x|y"   a box that scrolls while dragging near its edge:
 *                            the root or anything between it and a list
 *   data-drag-handle         inside an item: only this part starts a drag
 *   data-drag-carried        the stand-in the page draws for the item while
 *                            the keyboard carries it; it receives the focus
 *   data-drag-scope="<name>" on a list, an item, a zone or a stand-in: which
 *                            instance it belongs to (see below)
 *
 * The item, or its handle when it has one, must be focusable: the keyboard
 * picks an item up from there.
 *
 * Nothing is moved in the data until the drop: `active` says which items are
 * on their way and what letting go would do, and the page draws that (the
 * items hidden where they were, a gap at `active.gap`, the grabbed item at
 * `overlay`). A dragged item must stay in the document, hidden, for as long
 * as it is dragged: a finger's touch events are sent to the element it landed
 * on.
 *
 * Two levels in one root. Several instances can share a root, each with its
 * own `scope`: the columns of a board sorted by one, the cards in the columns
 * by another. An instance reads only the lists, items and zones marked with
 * its scope (those without the attribute, when it has no scope), and an item
 * belongs to the nearest list of its scope around it. A press belongs to the
 * innermost item under it, of whatever scope, so one press never starts two
 * drags: pressing a card drags the card, pressing the heading of its column
 * (the column's handle) drags the column.
 */
import { tick } from 'svelte';

import {
  carriedItems,
  dropChanges,
  dropRegion,
  dropTarget,
  gapFor,
  intersectRect,
  keyboardDestination,
  locateItem,
  sameDestination,
  type Axis,
  type Destination,
  type DropTarget,
  type KeyboardStep,
  type ListOrder,
  type Placement,
  type Point,
  type Rect,
} from '$lib/logic/drop-target.ts';

import { AutoScroller, type AutoScrollOptions, type ScrollBox } from './auto-scroll.ts';
import {
  pointerDrag,
  type PointerDragHandle,
  type PointerDragOptions,
  type PointerDragSession,
} from './pointer-drag.ts';
import { DRAG_SETTINGS } from './settings.ts';

const ITEM = 'data-drag-item';
const LIST = 'data-drag-list';
const ZONE = 'data-drag-zone';
const SCROLL = 'data-drag-scroll';
const HANDLE = 'data-drag-handle';
const CARRIED = 'data-drag-carried';
const SCOPE = 'data-drag-scope';

/** Controls inside an item keep their own presses. */
const CONTROLS =
  'input, textarea, select, button, a[href], [contenteditable]:not([contenteditable="false"])';

/** A space that is not shown and not trimmed away: it makes the same announcement a new one. */
const NO_BREAK_SPACE = String.fromCodePoint(0xa0);

/**
 * A drop that changed something.
 *
 * `to` says what the drop means, and only a drop at a place has an index:
 * into a list that places items itself (`kind: 'list'`) and onto a zone
 * (`kind: 'zone'`) there is none, and where the items end up is for the
 * caller to decide.
 */
export interface SortableMove {
  /** The item that was grabbed. */
  itemId: string;
  /**
   * Everything that was dropped, in the order of the data: the grabbed item
   * alone, or the selection it was part of. `moveItems` takes it as it is.
   */
  itemIds: readonly string[];
  /** Where the grabbed item was, counted without the other dropped items. */
  from: DropTarget;
  to: Destination;
}

/** What is on its way. */
export interface SortableActive {
  /** The item that was grabbed. */
  itemId: string;
  /** Everything on its way, in the order of the data. One id unless a selection is dragged. */
  itemIds: readonly string[];
  mode: 'pointer' | 'keyboard';
  /** Who places the item in a list: the pointer or keys (`free`), or the list (`list`). */
  placement: Placement;
  /** Where the grabbed item was, counted without the others on their way. */
  from: DropTarget;
  /** What letting go now would do. Undefined while the pointer is outside the root: nothing, the drag is cancelled. */
  to: Destination | undefined;
  /**
   * Where the page shows the gap: at the place of the drop; where the list's
   * order would put the item when the list places items itself; where the
   * item came from while the pointer is outside. Undefined over a zone.
   */
  gap: DropTarget | undefined;
  /** The pointer is outside the root: letting go cancels. */
  outside: boolean;
  /** The size of the grabbed item when it was picked up, for the gap and the overlay. */
  width: number;
  height: number;
}

/** A place in words: for the announcements. `position` starts at 1. */
export interface SortablePlace {
  /** The name of the grabbed item. */
  item: string;
  /** How many items are on their way: 1 unless a selection is dragged. */
  items: number;
  list: string;
  position: number;
  count: number;
}

/** A zone in words. */
export interface SortableZonePlace {
  item: string;
  items: number;
  zone: string;
}

/**
 * The texts. The first four are read out to a screen reader; `sorted` and
 * `outside` are also shown on the page while they apply (`notice`).
 */
export interface SortableMessages {
  pickedUp(place: SortablePlace): string;
  moved(place: SortablePlace): string;
  dropped(place: SortablePlace): string;
  cancelled(place: SortablePlace): string;
  /**
   * That the view is sorted, so a dropped item goes where the order puts it
   * and only the list can be chosen. Needed where `placement` can be `list`.
   */
  sorted?(): string;
  /** That letting go now cancels: the pointer is outside the root. */
  outside?(): string;
  /** Needed where there are zones. */
  movedToZone?(place: SortableZonePlace): string;
  droppedInZone?(place: SortableZonePlace): string;
}

export interface SortableOptions {
  /** The order as data, as the page shows it, at the time of the call. */
  order: () => readonly ListOrder[];
  /** Called once per drop that changes something. */
  onDrop: (move: SortableMove) => void;
  messages: SortableMessages;
  /** Names for the announcements. Default: the ids. */
  itemLabel?: (itemId: string) => string;
  listLabel?: (listId: string) => string;
  zoneLabel?: (zoneId: string) => string;
  /** The direction in which each list stacks its items. Default `y`. */
  axis?: Axis;
  /**
   * Which of the marked elements under the root are this instance's: those
   * with `data-drag-scope="<scope>"`. Without a scope, those that have no
   * such attribute.
   */
  scope?: string;
  /**
   * Who places a dropped item in its list, asked when an item is picked up.
   * `list`: the view is sorted, so only the list can be chosen. Default `free`.
   */
  placement?: () => Placement;
  /**
   * With `placement` `list`: the index, counted without the items on their
   * way (`carried`), at which the order of that list would put the grabbed
   * item. The gap is drawn there. Without it the gap is at the end of the list.
   */
  sortedIndex?: (itemId: string, listId: string, carried: readonly string[]) => number | undefined;
  /**
   * The items selected now. When the grabbed item is one of them the drag
   * carries them all; otherwise it carries the grabbed item alone.
   */
  selection?: () => readonly string[];
  /** The ids of the zones the keyboard can reach, in order: they come after the last list. */
  zones?: () => readonly string[];
  pointer?: Partial<PointerDragOptions>;
  scroll?: Partial<AutoScrollOptions>;
}

function viewport(): Rect {
  return { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight };
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
  /** The pointer or something under it moved since the destination was last worked out. */
  #stale = false;
  #frame: number | undefined;
  #frameTime = 0;
  /** What had the focus when the item was picked up, and gets it back: the item or its handle. */
  #focused: 'item' | 'handle' | undefined;
  /** The last index the gap had in a list: where the keyboard comes back to from a zone. */
  #resumeIndex = 0;

  constructor(options: SortableOptions) {
    this.#options = options;
    this.#scroller = new AutoScroller(options.scroll);
  }

  /** What is on its way, or undefined when nothing is dragged. */
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

  /**
   * What the page shows in words while it applies, and nothing otherwise:
   * that letting go now cancels, or that the view is sorted and only the
   * list can be chosen.
   */
  get notice(): string {
    const active = this.#active;
    if (!active) return '';
    if (active.outside) return this.#options.messages.outside?.() ?? '';
    if (active.to?.kind === 'list') return this.#options.messages.sorted?.() ?? '';
    return '';
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

  // ------------------------------------------------------------ the page

  /** Matches the elements of this instance that carry `attribute`, with `value` when given. */
  #selector(attribute: string, value?: string): string {
    const marked = value === undefined ? `[${attribute}]` : `[${attribute}="${CSS.escape(value)}"]`;
    const scope = this.#options.scope;
    return scope === undefined
      ? `${marked}:not([${SCOPE}])`
      : `${marked}[${SCOPE}="${CSS.escape(scope)}"]`;
  }

  /**
   * This instance's elements that carry any of the attributes, the root
   * included, in the order of the document. One walk through the root,
   * however many attributes are asked for.
   */
  #all(...attributes: string[]): HTMLElement[] {
    const root = this.#root;
    if (!root) return [];
    const selector = attributes.map((attribute) => this.#selector(attribute)).join(', ');
    const inside = [...root.querySelectorAll<HTMLElement>(selector)];
    return root.matches(selector) ? [root, ...inside] : inside;
  }

  /** The first of this instance's elements with that attribute and value: the root, or one in it. */
  #first(attribute: string, value?: string): HTMLElement | undefined {
    const root = this.#root;
    if (!root) return undefined;
    const selector = this.#selector(attribute, value);
    return root.matches(selector) ? root : (root.querySelector<HTMLElement>(selector) ?? undefined);
  }

  /**
   * The items of a list: those of this scope whose nearest list is this one,
   * not those of a list within. `nested` says whether there is a list within
   * at all: this runs on every frame of a drag, over every item of a long
   * list, and the walk up from each item is only made when it has to be.
   */
  #itemsOf(list: HTMLElement, nested: boolean): HTMLElement[] {
    const items = [...list.querySelectorAll<HTMLElement>(this.#selector(ITEM))];
    if (!nested) return items;
    const lists = this.#selector(LIST);
    return items.filter((item) => item.closest(lists) === list);
  }

  /** The handles of an item: not those of items within it. */
  #handlesOf(item: HTMLElement): HTMLElement[] {
    return [...item.querySelectorAll<HTMLElement>(`[${HANDLE}]`)].filter(
      (handle) => handle.closest(`[${ITEM}]`) === item,
    );
  }

  #isMine(element: Element): boolean {
    return (element.getAttribute(SCOPE) ?? undefined) === this.#options.scope;
  }

  /** Right to left: a row of items runs against the x coordinate, and the arrow keys swap. */
  #rtl(): boolean {
    return this.#root !== undefined && getComputedStyle(this.#root).direction === 'rtl';
  }

  // -------------------------------------------------------------- pick up

  /** The state of a drag that begins now with `item`, or undefined when the data no longer has it. */
  #pickUp(
    itemId: string,
    mode: SortableActive['mode'],
    item: HTMLElement,
  ): SortableActive | undefined {
    const order = this.#options.order();
    if (!locateItem(order, itemId)) return undefined;
    const itemIds = carriedItems(order, itemId, this.#options.selection?.());
    const from = locateItem(order, itemId, itemIds);
    if (!from) return undefined;
    const placement = this.#options.placement?.() ?? 'free';
    const to: Destination =
      placement === 'list' ? { kind: 'list', listId: from.listId } : { kind: 'index', ...from };
    const { width, height } = item.getBoundingClientRect();
    this.#resumeIndex = from.index;
    return { itemId, itemIds, mode, placement, from, to, gap: from, outside: false, width, height };
  }

  /** Points the drag at a new destination. Returns whether that changed anything. */
  #aim(to: Destination | undefined): boolean {
    const active = this.#active;
    if (!active || sameDestination(to, active.to)) return false;
    const gap = gapFor(to, {
      from: active.from,
      lists: this.#options.order(),
      carried: active.itemIds,
      sortedIndex: (listId) => this.#options.sortedIndex?.(active.itemId, listId, active.itemIds),
    });
    if (gap) this.#resumeIndex = gap.index;
    this.#active = { ...active, to, gap, outside: to === undefined };
    return true;
  }

  // ------------------------------------------------------------- pointer

  readonly #press = (event: PointerEvent): PointerDragSession | undefined => {
    if (this.#active) return undefined;
    const item = this.#itemAt(event.target);
    const itemId = item?.getAttribute(ITEM);
    if (!item || !itemId) return undefined;
    if (!locateItem(this.#options.order(), itemId)) return undefined;

    const pressed = item.getBoundingClientRect();
    const grab = { x: event.clientX - pressed.left, y: event.clientY - pressed.top };
    return {
      start: (point) => {
        const active = this.#pickUp(itemId, 'pointer', item);
        if (!active) return;
        this.#focused = this.#focusOn(item);
        this.#grab = grab;
        this.#point = point;
        this.#active = active;
        this.#overlay = { x: point.x - grab.x, y: point.y - grab.y };
        this.#root?.addEventListener('scroll', this.#onScroll, { capture: true, passive: true });
        this.#stale = true;
        this.#frameTime = performance.now();
        this.#frame = requestAnimationFrame(this.#onFrame);
        if (active.placement === 'list') this.#say(this.#options.messages.sorted?.() ?? '');
      },
      move: (point) => {
        this.#point = point;
        if (!this.#active) return;
        this.#overlay = { x: point.x - this.#grab.x, y: point.y - this.#grab.y };
        this.#stale = true;
      },
      drop: (point) => {
        this.#point = point;
        this.#retarget();
        // Outside the root there is no destination, and this is a cancel.
        this.#finish(true);
      },
      cancel: () => {
        this.#finish(false);
      },
    };
  };

  /**
   * The item a press on `target` would drag, if it is this instance's. The
   * press belongs to the innermost item around it, of whatever scope.
   */
  #itemAt(target: EventTarget | null): HTMLElement | undefined {
    if (!(target instanceof Element)) return undefined;
    const item = target.closest<HTMLElement>(`[${ITEM}]`);
    if (!item || !this.#root?.contains(item) || !this.#isMine(item)) return undefined;
    const control = target.closest(CONTROLS);
    if (control && control !== item && item.contains(control)) return undefined;
    const handles = this.#handlesOf(item);
    if (handles.length > 0 && !handles.some((handle) => handle.contains(target))) return undefined;
    return item;
  }

  /** Whether the focus is on the item or on its handle now. */
  #focusOn(item: HTMLElement): 'item' | 'handle' | undefined {
    const focused = document.activeElement;
    if (focused === item) return 'item';
    return this.#handlesOf(item).some((handle) => handle === focused) ? 'handle' : undefined;
  }

  readonly #onScroll = (): void => {
    this.#stale = true;
  };

  /**
   * Once per frame while a pointer drags: the destination follows the
   * pointer, the boxes scroll, and the destination follows what scrolled
   * under the pointer.
   */
  readonly #onFrame = (now: number): void => {
    if (this.#active?.mode !== 'pointer') return;
    const elapsed = Math.min(now - this.#frameTime, DRAG_SETTINGS.longestFrame);
    this.#frameTime = now;
    if (this.#stale) this.#retarget();
    if (this.#scroller.step(this.#point, this.#scrollBoxes(), elapsed)) this.#retarget();
    this.#frame = requestAnimationFrame(this.#onFrame);
  };

  /** Works out the destination from where the pointer and the items are now. */
  #retarget(): void {
    const root = this.#root;
    const active = this.#active;
    if (!root || !active) return;
    this.#stale = false;

    // The boxes of the lists and zones first, and then the items of the one
    // list the pointer picks: the cost of a frame does not grow with the
    // other lists.
    const bounds = intersectRect(root.getBoundingClientRect(), viewport());
    const axis = this.#options.axis;
    const lists: { element: HTMLElement; id: string; rect: DOMRect }[] = [];
    const zones: { id: string; rect: DOMRect }[] = [];
    for (const element of this.#all(LIST, ZONE)) {
      const rect = element.getBoundingClientRect();
      const listId = element.getAttribute(LIST);
      if (listId === null) zones.push({ id: element.getAttribute(ZONE) ?? '', rect });
      else lists.push({ element, id: listId, rect });
    }
    const region = dropRegion(this.#point, { bounds, zones, lists, axis });
    if (!region) return;

    if (region.kind === 'outside') {
      this.#aim(undefined);
    } else if (region.kind === 'zone') {
      this.#aim({ kind: 'zone', zoneId: region.zoneId });
    } else if (active.placement === 'list') {
      this.#aim({ kind: 'list', listId: region.list.id });
    } else {
      const { element, id, rect } = region.list;
      const nested = lists.some(
        (other) => other.element !== element && element.contains(other.element),
      );
      const items = this.#itemsOf(element, nested)
        .filter((item) => !active.itemIds.includes(item.getAttribute(ITEM) ?? ''))
        .map((item) => item.getBoundingClientRect());
      const reversed = axis === 'x' && this.#rtl();
      const place = dropTarget(this.#point, [{ id, rect, items }], { axis, bounds, reversed });
      if (place) this.#aim({ kind: 'index', ...place });
    }
  }

  /**
   * The boxes that may scroll now: those around the list or zone the item
   * would land in. None while the pointer is outside the root: there a
   * release cancels, and a board that kept scrolling would say otherwise.
   */
  #scrollBoxes(): ScrollBox[] {
    const root = this.#root;
    const to = this.#active?.to;
    if (!root || !to) return [];
    const boxes: ScrollBox[] = [];
    let element = to.kind === 'zone' ? this.#first(ZONE, to.zoneId) : this.#first(LIST, to.listId);
    while (element && root.contains(element)) {
      const axis = element.getAttribute(SCROLL);
      if (axis === 'x' || axis === 'y') boxes.push({ element, axis });
      element = element.parentElement ?? undefined;
    }
    return boxes;
  }

  // ------------------------------------------------------------ keyboard

  /** Space on a focused item, or on the focused handle of one, picks it up. */
  readonly #onKeyDown = (event: KeyboardEvent): void => {
    if (this.#active || event.key !== ' ' || event.repeat) return;
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const onHandle = !target.hasAttribute(ITEM) && target.hasAttribute(HANDLE);
    const item = onHandle ? target.closest<HTMLElement>(`[${ITEM}]`) : target;
    if (!item?.hasAttribute(ITEM) || !this.#isMine(item)) return;
    const active = this.#pickUp(item.getAttribute(ITEM) ?? '', 'keyboard', item);
    if (!active) return;

    event.preventDefault();
    this.#focused = onHandle ? 'handle' : 'item';
    this.#active = active;
    // On the window and capturing: the focus sits on the stand-in, which the
    // page may draw anywhere, and no other handler may act on these keys.
    window.addEventListener('keydown', this.#onCarryKey, true);
    window.addEventListener('pointerdown', this.#onCarryPointer, true);
    const sorted = active.placement === 'list' ? this.#options.messages.sorted?.() : undefined;
    this.#say([this.#words('pickedUp', active), sorted].filter(Boolean).join(' '));
    void this.#focusCarried();
  };

  readonly #onCarryKey = (event: KeyboardEvent): void => {
    const active = this.#active;
    if (active?.mode !== 'keyboard' || !active.to) return;
    const step = this.#stepFor(event.key);
    if (step) {
      const to = keyboardDestination(active.to, step, {
        lists: this.#options.order(),
        zones: this.#options.zones?.(),
        carried: active.itemIds,
        placement: active.placement,
        resumeIndex: this.#resumeIndex,
      });
      if (this.#aim(to)) {
        if (this.#active) this.#say(this.#words('moved', this.#active));
        void this.#focusCarried();
      } else if (active.to.kind === 'list' && step !== 'previousList' && step !== 'nextList') {
        // A key that would move within the list, where the list decides: say so again.
        this.#say(this.#options.messages.sorted?.() ?? '');
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
    const vertical = (this.#options.axis ?? 'y') === 'y';
    switch (key) {
      case 'ArrowUp':
        return vertical ? 'previous' : 'previousList';
      case 'ArrowDown':
        return vertical ? 'next' : 'nextList';
      case 'ArrowLeft':
      case 'ArrowRight': {
        const forward = (key === 'ArrowRight') !== this.#rtl();
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
    const carried = this.#first(CARRIED);
    if (!carried) return;
    if (document.activeElement !== carried) carried.focus();
    carried.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  // ---------------------------------------------------------------- both

  /** Ends the drag. A commit without a destination (the pointer is outside the root) is a cancel. */
  #finish(commit: boolean): void {
    const active = this.#active;
    if (!active) return;

    if (this.#frame !== undefined) cancelAnimationFrame(this.#frame);
    this.#frame = undefined;
    this.#root?.removeEventListener('scroll', this.#onScroll, { capture: true });
    window.removeEventListener('keydown', this.#onCarryKey, true);
    window.removeEventListener('pointerdown', this.#onCarryPointer, true);

    const to = commit ? active.to : undefined;
    // A drop keeps the boxes where the drag scrolled them to; a cancel puts them back.
    if (to) this.#scroller.clear();
    else this.#scroller.restore();
    this.#say(this.#words(to ? 'dropped' : 'cancelled', active));
    this.#active = undefined;
    this.#overlay = undefined;
    if (to && dropChanges(this.#options.order(), active.itemIds, to)) {
      this.#options.onDrop({
        itemId: active.itemId,
        itemIds: active.itemIds,
        from: active.from,
        to,
      });
    }
    const focused = this.#focused;
    this.#focused = undefined;
    if (active.mode === 'keyboard') void this.#focusItem(active.itemId, focused, false);
    else if (focused) void this.#focusItem(active.itemId, focused, true);
  }

  async #focusItem(
    itemId: string,
    part: 'item' | 'handle' | undefined,
    preventScroll: boolean,
  ): Promise<void> {
    await tick();
    const item = this.#first(ITEM, itemId);
    if (!item) return;
    const target = part === 'handle' ? (this.#handlesOf(item)[0] ?? item) : item;
    target.focus({ preventScroll });
  }

  /** The words for what just happened to the drag. */
  #words(kind: 'pickedUp' | 'moved' | 'dropped' | 'cancelled', active: SortableActive): string {
    const { messages } = this.#options;
    const order = this.#options.order();
    const item = this.#options.itemLabel?.(active.itemId) ?? active.itemId;
    const items = active.itemIds.length;

    if (kind !== 'cancelled' && active.to?.kind === 'zone') {
      const place: SortableZonePlace = {
        item,
        items,
        zone: this.#options.zoneLabel?.(active.to.zoneId) ?? active.to.zoneId,
      };
      const words =
        kind === 'dropped' ? messages.droppedInZone?.(place) : messages.movedToZone?.(place);
      return words ?? '';
    }

    // A cancelled item is back where it is in the data, among all the items there.
    const home = kind === 'cancelled' ? locateItem(order, active.itemId) : undefined;
    const at = home ?? active.gap ?? active.from;
    const list = order.find((candidate) => candidate.id === at.listId);
    const staying = list ? list.items.filter((id) => !active.itemIds.includes(id)).length : 0;
    return messages[kind]({
      item,
      items,
      list: this.#options.listLabel?.(at.listId) ?? at.listId,
      position: at.index + 1,
      count: home ? (list?.items.length ?? 0) : staying + items,
    });
  }

  /**
   * Puts words in the live region. A live region reads only what changes, so
   * the same words twice in a row are told apart by a space that does not show.
   */
  #say(text: string): void {
    if (text === '') return;
    this.#announcement = text === this.#announcement ? text + NO_BREAK_SPACE : text;
  }
}
