/**
 * The facts a screen is laid out by. None of them is the User-Agent.
 *
 * - `narrow`: the viewport is under 768 CSS pixels wide. Decides the
 *   arrangement: drawer or fixed sidebar, sheet or popover, page or dialog.
 *   The same width the generated sidebar switches at, and Tailwind's `md`.
 * - `wide`: 1024 pixels or more. Only from here is there room, next to the
 *   sidebar, for the whole header of a view; between the two widths (a
 *   tablet held upright) the sidebar is fixed and the header is still folded.
 * - `coarse`: the main pointer is a finger. Decides sizes only: anything to
 *   be hit grows to `--spacing-touch`. In markup this is the `pointer-coarse:`
 *   variant; the value here is for the few places that need it in script.
 *
 * A narrow window on a desktop is narrow and fine; a tablet is wide and coarse.
 */
import { MediaQuery } from 'svelte/reactivity';

export const NARROW_BELOW = 768;
export const WIDE_FROM = 1024;

export const narrow = new MediaQuery(`max-width: ${NARROW_BELOW - 1}px`);
export const wide = new MediaQuery(`min-width: ${WIDE_FROM}px`);
export const coarse = new MediaQuery('pointer: coarse');
