/**
 * What a screen is laid out by. None of it is the User-Agent, and none of it
 * is a guess at the kind of device.
 *
 * - **Room.** How wide the box is that a part sits in. Two boxes are
 *   measured: `app`, the page less what the device keeps at its left and
 *   right edges, and `pane`, what is left of that for the board. The widths
 *   at which the layout changes are written once, in mobile.css; classes use
 *   them as container queries, and this file reads the same variables for
 *   what a class cannot decide (which component draws a card, what goes
 *   behind "…").
 * - **Pointer.** Whether the main pointer is a finger decides sizes only:
 *   anything to be hit grows to `--spacing-touch`. Whether the device can
 *   hover decides what may stay hidden until pointed at. In markup these are
 *   the `pointer-coarse:` and `can-hover:` variants; `coarse` here is for the
 *   one place that needs it in script. Width says nothing about either: a
 *   tablet is wide and takes a finger, a small window on a desktop is narrow
 *   and takes a mouse.
 *
 * Nothing here is read once and kept. A box is measured whenever it changes
 * (a fold opens, the window is split, the device turns), and a width from
 * mobile.css is turned into pixels at the moment it is compared.
 */
import { MediaQuery } from 'svelte/reactivity';

export type Stage = 'compact' | 'medium' | 'expanded';

const root = document.documentElement;

/** A width from mobile.css, in pixels at the reader's text size. */
function boundary(name: string): number {
  const value = getComputedStyle(root).getPropertyValue(name).trim();
  if (!value.endsWith('rem')) throw new Error(`mobile.css does not give ${name} in rem`);
  return parseFloat(value) * parseFloat(getComputedStyle(root).fontSize);
}

/** A length in rem, in pixels at the reader's text size. */
export function rem(length: number): number {
  return length * parseFloat(getComputedStyle(root).fontSize);
}

/** The width of the content of one element, kept up to date. */
class Measured {
  /** 0 until the element is on the page. */
  width = $state(0);

  constructor(initial = 0) {
    this.width = initial;
  }

  /** Follows an element from now on. For `{@attach}`. */
  readonly attach = (element: HTMLElement): (() => void) => {
    const style = getComputedStyle(element);
    this.width =
      element.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
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

class AppRoom extends Measured {
  readonly stage = $derived<Stage>(
    this.width >= boundary('--container-expanded')
      ? 'expanded'
      : this.width >= boundary('--container-medium')
        ? 'medium'
        : 'compact',
  );
}

class PaneRoom extends Measured {
  /** False while the pane has not been measured: nothing should act on a width of 0. */
  readonly measured = $derived(this.width > 0);
  /** Two columns of a kanban fit side by side. */
  readonly columns = $derived(this.width >= boundary('--container-columns'));
  /** The whole header of a view fits in one row. */
  readonly header = $derived(this.width >= boundary('--container-header'));
}

/** The room the device keeps at each edge, in pixels: for parts that are placed by script. */
class Insets {
  top = $state(0);
  right = $state(0);
  bottom = $state(0);
  left = $state(0);
  readonly #probe = document.createElement('div');

  constructor() {
    this.#probe.style.cssText =
      'position:fixed;visibility:hidden;pointer-events:none;' +
      'padding:var(--safe-top) var(--safe-right) var(--safe-bottom) var(--safe-left)';
    document.body.append(this.#probe);
    this.refresh();
    // The values change when the device turns or folds, and the window changes with them.
    window.addEventListener('resize', this.refresh);
  }

  readonly refresh = (): void => {
    const style = getComputedStyle(this.#probe);
    this.top = parseFloat(style.paddingTop);
    this.right = parseFloat(style.paddingRight);
    this.bottom = parseFloat(style.paddingBottom);
    this.left = parseFloat(style.paddingLeft);
  };
}

const page = document.getElementById('app');

/** The page less the edges the device keeps. Known before anything is drawn. */
export const app = new AppRoom();
if (page) app.attach(page);

/** What the board has. The shell attaches it. */
export const pane = new PaneRoom();

export const insets = new Insets();

export const coarse = new MediaQuery('pointer: coarse');
