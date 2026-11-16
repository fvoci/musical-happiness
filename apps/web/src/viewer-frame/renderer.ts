/**
 * What a renderer is to the frame: something that draws a file's bytes into
 * a place and then follows the viewer's commands.
 */

/** What a renderer tells the viewer around it. */
export interface Report {
  /** The document is drawn as far as the reader can see it, and has this many pages. */
  shown(pages: number): void;
  /** The page most in view is now this one (from 1). */
  page(page: number): void;
}

export interface Drawn {
  /** Brings a page (from 1) into view. */
  goTo(page: number): void;
  /** Draws at this zoom, 1 being the document's own size. */
  zoom(zoom: number): void;
}

/** Thrown when the file is a kind the renderer reads, but locked with a password. */
export class LockedFile extends Error {}

/** Thrown when the bytes are not a readable file of the kind. */
export class UnreadableFile extends Error {}

export type Render = (bytes: ArrayBuffer, place: HTMLElement, report: Report) => Promise<Drawn>;

/**
 * Follows which page is most in view while the document scrolls, and tells
 * the viewer when that changes. `pages` are the page elements in order.
 */
export function followPages(pages: () => readonly Element[], report: Report): () => void {
  let current = 0;
  let frame = 0;
  const measure = (): void => {
    frame = 0;
    const middle = window.innerHeight / 2;
    let best = 1;
    let bestDistance = Infinity;
    pages().forEach((page, index) => {
      const box = page.getBoundingClientRect();
      const distance =
        box.top <= middle && box.bottom >= middle
          ? 0
          : Math.min(Math.abs(box.top - middle), Math.abs(box.bottom - middle));
      if (distance < bestDistance) {
        bestDistance = distance;
        best = index + 1;
      }
    });
    if (best !== current) {
      current = best;
      report.page(best);
    }
  };
  const onScroll = (): void => {
    if (frame === 0) frame = requestAnimationFrame(measure);
  };
  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('resize', onScroll, { passive: true });
  measure();
  return onScroll;
}
