/**
 * One drag with a pointer, from press to release, on Pointer Events.
 *
 * This part knows nothing about lists or drop targets. It decides when a
 * press becomes a drag, follows the pointer wherever it goes, and makes sure
 * the drag ends exactly once: with a drop or with a cancel.
 *
 * A mouse starts dragging once it has moved a few pixels with the button
 * down, so a click stays a click. A finger (or a pen) starts dragging after
 * resting on the item for a moment; a finger that moves before that is
 * scrolling, and is left to the browser. The moment the resting finger has
 * picked the item up the device buzzes briefly, where it can.
 */
import type { Point } from '$lib/logic/drop-target.ts';

import { DRAG_SETTINGS } from './settings.ts';

export interface PointerDragOptions {
  /** How far a mouse travels with the button down before the drag begins, in pixels. */
  mouseDistance: number;
  /** How long a finger rests before the drag begins, in milliseconds. */
  touchDelay: number;
  /** How far a resting finger may wander without giving up the drag, in pixels. */
  touchTolerance: number;
  /** How long the device buzzes when a resting finger begins a drag, in milliseconds. 0 for none. */
  touchVibration: number;
}

export const POINTER_DRAG_DEFAULTS: PointerDragOptions = {
  mouseDistance: DRAG_SETTINGS.mouseDistance,
  touchDelay: DRAG_SETTINGS.touchDelay,
  touchTolerance: DRAG_SETTINGS.touchTolerance,
  touchVibration: DRAG_SETTINGS.touchVibration,
};

/** What one drag does. `start` is called once; after it exactly one of `drop` and `cancel`. */
export interface PointerDragSession {
  start(point: Point): void;
  move(point: Point): void;
  drop(point: Point): void;
  cancel(): void;
}

/**
 * Asked on every press of the primary button or first finger inside the
 * root. Returns the session for the thing under the pointer, or undefined
 * when there is nothing to drag there. Must not change anything yet: most
 * presses end as a click or a scroll.
 */
export type PointerDragSource = (event: PointerEvent) => PointerDragSession | undefined;

export interface PointerDragHandle {
  /** Cancels the drag in progress, if any. */
  cancel(): void;
  /** Cancels and removes every listener. */
  destroy(): void;
}

interface Press {
  session: PointerDragSession;
  pointerId: number;
  /** A finger or pen: starts by resting, and the browser may take the gesture for scrolling. */
  touch: boolean;
  origin: Point;
  last: Point;
  dragging: boolean;
  timer: ReturnType<typeof setTimeout> | undefined;
  /** Removes the listeners that only live while this press does. */
  release: () => void;
}

function pointOf(event: PointerEvent): Point {
  return { x: event.clientX, y: event.clientY };
}

/**
 * A short buzz, on devices that have one. Safari on iOS has no
 * `navigator.vibrate` at all, and a browser may refuse (before the page was
 * ever tapped, or by a setting of the device): neither is an error here.
 */
function buzz(duration: number): void {
  if (duration <= 0 || !('vibrate' in navigator)) return;
  try {
    navigator.vibrate(duration);
  } catch {
    // Nothing to do: the drag goes on without the buzz.
  }
}

export function pointerDrag(
  root: HTMLElement,
  source: PointerDragSource,
  options: Partial<PointerDragOptions> = {},
): PointerDragHandle {
  const settings = { ...POINTER_DRAG_DEFAULTS, ...options };
  const NOT_PASSIVE = { passive: false };
  // Capturing listeners on the window see the pointer before anything inside
  // the page can stop the event.
  const CAPTURE = { capture: true };
  let press: Press | undefined;
  /** A pointer whose drag was cancelled while it was still down. */
  let spent: { touch: boolean; forget: () => void } | undefined;

  function finish(): Press | undefined {
    const ended = press;
    if (!ended) return undefined;
    press = undefined;
    clearTimeout(ended.timer);
    ended.release();
    if (ended.dragging && root.hasPointerCapture(ended.pointerId)) {
      root.releasePointerCapture(ended.pointerId);
    }
    return ended;
  }

  /** `down`: the pointer is still pressed, as after Escape, and not gone, as after pointercancel. */
  function cancel(down: boolean): void {
    const ended = finish();
    if (!ended?.dragging) return;
    if (down) ignoreUntilLifted(ended);
    ended.session.cancel();
  }

  /**
   * The rest of a cancelled press does nothing: the finger still on the
   * screen does not scroll, and the button going up is not a click.
   */
  function ignoreUntilLifted(ended: Press): void {
    const lifted = (event: PointerEvent): void => {
      if (event.pointerId !== ended.pointerId) return;
      if (event.type === 'pointerup') swallowNextClick();
      forget();
    };
    const forget = (): void => {
      window.removeEventListener('pointerup', lifted, CAPTURE);
      window.removeEventListener('pointercancel', lifted, CAPTURE);
      spent = undefined;
    };
    window.addEventListener('pointerup', lifted, CAPTURE);
    window.addEventListener('pointercancel', lifted, CAPTURE);
    spent = { touch: ended.touch, forget };
  }

  function begin(): void {
    if (!press || press.dragging) return;
    clearTimeout(press.timer);
    press.dragging = true;
    // The root takes the pointer: the item under it may leave the document
    // while it is dragged, and events must keep coming when the pointer
    // leaves the window.
    try {
      root.setPointerCapture(press.pointerId);
    } catch {
      // The pointer is already gone; its pointerup or pointercancel follows.
    }
    // Only a finger that rested gets the buzz: it is the one sign that the
    // wait is over. A mouse sees the item move.
    if (press.touch) buzz(settings.touchVibration);
    press.session.start(press.last);
  }

  /** A drag that ends with the button going up would otherwise be followed by a click. */
  function swallowNextClick(): void {
    const swallow = (event: MouseEvent): void => {
      event.stopPropagation();
      event.preventDefault();
    };
    window.addEventListener('click', swallow, { capture: true, once: true });
    // The click, if one comes, is dispatched right after pointerup, before any timer.
    setTimeout(() => {
      window.removeEventListener('click', swallow, CAPTURE);
    }, 0);
  }

  function onPointerMove(event: PointerEvent): void {
    if (event.pointerId !== press?.pointerId) return;
    press.last = pointOf(event);
    if (press.dragging) {
      press.session.move(press.last);
      return;
    }
    const travelled = Math.hypot(press.last.x - press.origin.x, press.last.y - press.origin.y);
    if (press.touch) {
      // Moved before the wait was over: this is a scroll, not a drag.
      if (travelled > settings.touchTolerance) finish();
    } else if (travelled >= settings.mouseDistance) {
      begin();
    }
  }

  function onPointerUp(event: PointerEvent): void {
    if (event.pointerId !== press?.pointerId) return;
    const ended = finish();
    if (!ended?.dragging) return;
    swallowNextClick();
    ended.session.drop(pointOf(event));
  }

  function onPointerCancel(event: PointerEvent): void {
    if (event.pointerId === press?.pointerId) cancel(false);
  }

  function onLostCapture(event: PointerEvent): void {
    // After a regular pointerup the press is already over and this is ignored.
    if (event.pointerId === press?.pointerId && press.dragging) cancel(true);
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== 'Escape' || !press) return;
    if (press.dragging) {
      // Escape ends the drag and nothing else, such as a dialog around the board.
      event.preventDefault();
      event.stopPropagation();
    }
    cancel(true);
  }

  /**
   * Once a finger drags, the browser must not also scroll with it. Pointer
   * Events offer no way to say so after the touch has begun (`touch-action`
   * is read when the finger lands), so the touch event is cancelled instead.
   */
  function onTouchMove(event: Event): void {
    const held = press?.dragging ? press.touch : spent?.touch;
    if (held && event.cancelable) event.preventDefault();
  }

  function preventWhilePressed(event: Event): void {
    if (press) event.preventDefault();
  }

  /** A long press also opens the context menu on touch screens. */
  function onContextMenu(event: Event): void {
    if (press?.touch) event.preventDefault();
  }

  function onPointerDown(event: PointerEvent): void {
    if (press || !event.isPrimary || event.button !== 0) return;
    spent?.forget();
    const session = source(event);
    if (!session) return;

    const touch = event.pointerType !== 'mouse';
    const origin = pointOf(event);
    const target = event.target;

    window.addEventListener('pointermove', onPointerMove, CAPTURE);
    window.addEventListener('pointerup', onPointerUp, CAPTURE);
    window.addEventListener('pointercancel', onPointerCancel, CAPTURE);
    window.addEventListener('keydown', onKeyDown, CAPTURE);
    document.addEventListener('selectstart', preventWhilePressed);
    // Touch events go to the element the finger landed on even after it has
    // left the document, where they no longer reach the root.
    if (touch) target?.addEventListener('touchmove', onTouchMove, NOT_PASSIVE);

    press = {
      session,
      pointerId: event.pointerId,
      touch,
      origin,
      last: origin,
      dragging: false,
      timer: touch ? setTimeout(begin, settings.touchDelay) : undefined,
      release: () => {
        window.removeEventListener('pointermove', onPointerMove, CAPTURE);
        window.removeEventListener('pointerup', onPointerUp, CAPTURE);
        window.removeEventListener('pointercancel', onPointerCancel, CAPTURE);
        window.removeEventListener('keydown', onKeyDown, CAPTURE);
        document.removeEventListener('selectstart', preventWhilePressed);
        target?.removeEventListener('touchmove', onTouchMove);
      },
    };
  }

  root.addEventListener('pointerdown', onPointerDown);
  root.addEventListener('lostpointercapture', onLostCapture);
  // Present before any finger lands: a browser decides when the touch begins
  // whether a listener may cancel its moves.
  root.addEventListener('touchmove', onTouchMove, NOT_PASSIVE);
  root.addEventListener('contextmenu', onContextMenu);
  // Links and images would start the browser's own drag, which cancels the pointer.
  root.addEventListener('dragstart', preventWhilePressed);

  return {
    cancel() {
      cancel(true);
    },
    destroy() {
      cancel(false);
      spent?.forget();
      root.removeEventListener('pointerdown', onPointerDown);
      root.removeEventListener('lostpointercapture', onLostCapture);
      root.removeEventListener('touchmove', onTouchMove);
      root.removeEventListener('contextmenu', onContextMenu);
      root.removeEventListener('dragstart', preventWhilePressed);
    },
  };
}
