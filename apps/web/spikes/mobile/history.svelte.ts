/**
 * Makes the back button (and the back gesture) close what is open instead of
 * leaving the page.
 *
 * Opening adds one entry to the history; going back removes it and closes the
 * thing. When the thing is closed some other way (its own button, a tap
 * outside), the entry is taken back out so that the next "back" does what the
 * person expects. Things opened on top of one another close in reverse order.
 *
 * The product will have the router do this: a card is an address of its own.
 * This is the least that shows the behaviour in the mock-ups.
 */

const closers: (() => void)[] = [];
/** How many `popstate` events are our own doing and close nothing. */
let ownPops = 0;
let listening = false;

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
    history.pushState({ fvociOverlay: closers.length }, '');
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
