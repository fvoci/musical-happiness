/**
 * Makes the back button (and the back gesture) close what is open instead of
 * leaving the page.
 *
 * Opening adds one entry to the history; going back removes it and closes the
 * thing. When the thing is closed some other way (its own button, a tap
 * outside), the entry is taken back out so that the next "back" does what the
 * person expects. Things opened on top of one another close in reverse order.
 *
 * This is for what has no address of its own: the drawer, sheets. A card has
 * an address, and the router opens and closes it. The entries made here keep
 * the address they were made on, so the router sees no change in them.
 */

const closers: (() => void)[] = [];
/** How many `popstate` events are our own doing and close nothing. */
let ownPops = 0;
let listening = false;

const MARK = 'fvociOverlay';

/** Whether a history state is one of the entries this file makes. */
export function isOverlayEntry(state: unknown): boolean {
  return typeof state === 'object' && state !== null && MARK in state;
}

function onPopState(): void {
  if (ownPops > 0) {
    ownPops -= 1;
    return;
  }
  closers.pop()?.();
}

/** Call while a component is being set up. */
export function closeOnBack(isOpen: () => boolean, close: () => void): void {
  // `pre`: such effects run from the outside in. When a card and a sheet over
  // it are both open from the start, the card is entered first and the sheet
  // is what "back" closes first.
  $effect.pre(() => {
    if (!isOpen()) return;
    if (!listening) {
      window.addEventListener('popstate', onPopState);
      listening = true;
    }
    const closer = (): void => {
      close();
    };
    closers.push(closer);
    history.pushState({ ...(history.state as object | null), [MARK]: closers.length }, '');
    return () => {
      const at = closers.indexOf(closer);
      // Not in the list: the back button took it out, and the entry with it.
      if (at === -1) return;
      closers.splice(at, 1);
      ownPops += 1;
      history.back();
    };
  });
}

/**
 * Lets go of the entries of everything that is open, and says how many there
 * were. The router calls this when it goes somewhere else while a drawer or
 * a sheet is open (a board was chosen in the drawer): it then puts the new
 * address in place of the overlay's entry, and the overlay, when it closes a
 * moment later, leaves the history alone.
 */
export function releaseOverlayEntries(): number {
  const open = closers.length;
  closers.length = 0;
  return open;
}
