/**
 * Word documents (DOCX), with docx-preview.
 *
 * docx-preview turns the document into HTML: a `section` per page as Word
 * breaks them where the document says so. Parts it would draw as raw HTML
 * (`altChunk`) and comments are left out. Pictures and embedded fonts become
 * `blob:` addresses, which the frame's policy allows; anything the document
 * links to on the network stays undrawn.
 *
 * A document names its fonts ("맑은 고딕", "Calibri") and most readers have
 * few of them. Every font a document names is followed by the app's own, so
 * that Korean text falls back to Noto Sans KR rather than to whatever the
 * browser picks.
 */
import { renderAsync } from 'docx-preview';

import { PageFollower, UnreadableFile, type Drawn, type Render, type Report } from './renderer.ts';

/** What every font family of the document falls back to: the app's families, as the frame's style sheet has them. */
const FALLBACK = '"Noto Sans KR", sans-serif';
/** The room around the pages, in CSS pixels, at each side. */
const GUTTER = 16;

function withFallback(family: string): string {
  return family === '' || family.includes('Noto Sans KR') ? family : `${family}, ${FALLBACK}`;
}

/** Puts the fallback after each font family the rendered document names, in its style sheets and its elements. */
function fallBack(styles: HTMLElement, body: HTMLElement): void {
  for (const element of styles.querySelectorAll('style')) {
    for (const rule of element.sheet?.cssRules ?? []) {
      if (rule instanceof CSSStyleRule && rule.style.fontFamily) {
        rule.style.fontFamily = withFallback(rule.style.fontFamily);
      }
    }
  }
  for (const element of body.querySelectorAll<HTMLElement>('[style*="font-family"]')) {
    element.style.fontFamily = withFallback(element.style.fontFamily);
  }
}

class DocxView implements Drawn {
  readonly #place: HTMLElement;
  readonly #width: number;
  #zoom = 1;

  readonly #follower: PageFollower;

  constructor(place: HTMLElement, report: Report) {
    this.#place = place;
    const first = place.querySelector('section.docx');
    this.#width = first instanceof HTMLElement ? first.offsetWidth : 816;
    this.#layOut();
    this.#follower = new PageFollower(() => this.pages, report);
    let resizing: ReturnType<typeof setTimeout> | undefined;
    window.addEventListener('resize', () => {
      clearTimeout(resizing);
      resizing = setTimeout(() => {
        this.#layOut();
      }, 150);
    });
  }

  get pages(): Element[] {
    return [...this.#place.querySelectorAll('section.docx')];
  }

  #layOut(): void {
    const room = Math.max(1, window.innerWidth - 2 * GUTTER);
    this.#place.style.zoom = String(Math.min(1, room / this.#width) * this.#zoom);
  }

  goTo(page: number): void {
    const section = this.pages[page - 1];
    if (!section) return;
    section.scrollIntoView({ block: 'start' });
    this.#follower.pin(page);
  }

  zoom(zoom: number): void {
    const scroller = document.scrollingElement;
    const ratio = scroller ? scroller.scrollTop / Math.max(1, scroller.scrollHeight) : 0;
    this.#zoom = zoom;
    this.#layOut();
    if (scroller) scroller.scrollTop = ratio * scroller.scrollHeight;
  }
}

export const render: Render = async (bytes, place, report) => {
  const styles = document.createElement('div');
  const body = document.createElement('div');
  body.className = 'docx-body';
  place.replaceChildren(styles, body);
  try {
    await renderAsync(bytes, body, styles, {
      className: 'docx',
      inWrapper: true,
      breakPages: true,
      ignoreLastRenderedPageBreak: true,
      renderAltChunks: false,
      renderComments: false,
      renderChanges: false,
      useBase64URL: false,
      experimental: false,
    });
  } catch {
    throw new UnreadableFile();
  }
  fallBack(styles, body);
  const view = new DocxView(body, report);
  report.shown(Math.max(1, view.pages.length));
  return view;
};
