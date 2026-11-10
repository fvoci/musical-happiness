/**
 * Keeps the place a card was scrolled to while its box changes size.
 *
 * A browser keeps the scroll position of a box that is resized, except when
 * there is no longer that much to scroll: then it cuts the position down,
 * and does not give it back when the box gets its old size again. A card
 * that is scrolled 400 pixels down on a phone has nothing to scroll in a
 * wide dialog; folding the device shut again would leave it at the top.
 *
 * So the place the reader scrolled to is remembered together with the size
 * of the box it was scrolled in. A scroll event that only reports such a cut
 * does not replace it, and whenever the box changes size the card is put
 * back as near to the remembered place as it can go.
 *
 * The same watcher keeps a focused field in sight when only the height
 * shrinks, which is what the on-screen keyboard does.
 */

/** Where the reader scrolled to, and the size of the box at that moment. */
export interface Kept {
  top: number;
  width: number;
  height: number;
}

/** The box as it is now. `max` is how far it can be scrolled. */
export interface Box {
  top: number;
  width: number;
  height: number;
  max: number;
}

/**
 * What to remember after a scroll event. The reader's own scrolling is
 * remembered; a position the browser cut down because the box changed size
 * is not.
 */
export function afterScroll(kept: Kept, now: Box): Kept {
  const resized = now.width !== kept.width || now.height !== kept.height;
  const cut = resized && now.top < kept.top && now.top >= now.max - 1;
  return cut ? kept : { top: now.top, width: now.width, height: now.height };
}

/** Where to scroll after the box changed size; undefined to leave it. */
export function afterResize(kept: Kept, now: Box): number | undefined {
  const wanted = Math.min(kept.top, now.max);
  return wanted > now.top ? wanted : undefined;
}

function boxOf(element: HTMLElement): Box {
  return {
    top: element.scrollTop,
    width: element.clientWidth,
    height: element.clientHeight,
    max: element.scrollHeight - element.clientHeight,
  };
}

/** The rectangle of the caret in a focused editor, or of the focused field. */
function focusRect(scroller: HTMLElement): DOMRect | undefined {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !scroller.contains(active)) return undefined;
  if (!active.isContentEditable) return active.getBoundingClientRect();
  const selection = getSelection();
  const range = selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : undefined;
  if (!range || !active.contains(range.startContainer)) return active.getBoundingClientRect();
  const caret = range.getClientRects()[0];
  if (caret) return caret;
  // A caret on an empty line has no rectangle of its own: take the line.
  const node = range.startContainer;
  const line = node instanceof Element ? node : node.parentElement;
  return (line ?? active).getBoundingClientRect();
}

/** Room left between the focused field and the edge of the box, in pixels. */
const MARGIN = 12;

function revealFocus(scroller: HTMLElement): void {
  const rect = focusRect(scroller);
  if (!rect) return;
  const box = scroller.getBoundingClientRect();
  if (rect.bottom > box.bottom - MARGIN) scroller.scrollTop += rect.bottom - box.bottom + MARGIN;
  else if (rect.top < box.top + MARGIN) scroller.scrollTop -= box.top + MARGIN - rect.top;
}

/** For `{@attach}` on the box that scrolls. */
export function keepScroll(scroller: HTMLElement): () => void {
  let kept: Kept = boxOf(scroller);
  let last = { width: scroller.clientWidth, height: scroller.clientHeight };

  const onScroll = (): void => {
    kept = afterScroll(kept, boxOf(scroller));
  };
  scroller.addEventListener('scroll', onScroll, { passive: true });

  const observer = new ResizeObserver(() => {
    const now = boxOf(scroller);
    const back = afterResize(kept, now);
    if (back !== undefined) scroller.scrollTop = back;
    // Only lower, as wide as before: the keyboard came up over the page.
    if (now.width === last.width && now.height < last.height) revealFocus(scroller);
    last = { width: now.width, height: now.height };
  });
  observer.observe(scroller);

  return () => {
    scroller.removeEventListener('scroll', onScroll);
    observer.disconnect();
  };
}
