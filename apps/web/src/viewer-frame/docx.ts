/**
 * Word documents (DOCX), with docx-preview.
 *
 * docx-preview turns the document into HTML: a `section` per page, where
 * the document breaks them and where Word last broke them when it saved. Parts it would draw as raw HTML
 * (`altChunk`), comments and embedded fonts are left out. Pictures become
 * `blob:` addresses, which the frame's policy allows; anything the document
 * links to on the network stays undrawn.
 *
 * The archive it gets is the one the frame wrote of only the parts it
 * counted (`zip-guard.ts`). As it reads the document, docx-preview parses a
 * part at most twice (once for each spelling of its path), which the count
 * allows for. As it draws, it would unpack a picture again for each place
 * the document shows it, all at once; so its package is made to unpack each
 * part once from then on.
 *
 * A document names its fonts ("맑은 고딕", "Calibri") and most readers have
 * few of them. Every font a document names is followed by the app's own, so
 * that Korean text falls back to Noto Sans KR rather than to whatever the
 * browser picks.
 */
import { parseAsync, renderDocument, type Options } from 'docx-preview';

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

const OPTIONS = {
  className: 'docx',
  inWrapper: true,
  breakPages: true,
  // Pages break where Word last laid the document out, as well as where it says so:
  // otherwise a document without manual breaks is one long page.
  ignoreLastRenderedPageBreak: false,
  // Fonts are read as bytes the library changes in place, so they cannot be unpacked once
  // for all; the app's fonts stand in for them.
  ignoreFonts: true,
  // Its pattern for the declaration takes time that grows with the square of a line.
  trimXmlDeclaration: false,
  renderAltChunks: false,
  renderComments: false,
  renderChanges: false,
  useBase64URL: false,
  experimental: false,
} satisfies Partial<Options>;

/** What docx-preview's package is to this: how it finds a part, and unpacks one. */
interface Package {
  get(path: string): unknown;
  load(path: string, type?: string): Promise<unknown>;
}

function packageOf(document: unknown): Package | undefined {
  if (typeof document !== 'object' || document === null || !('_package' in document)) {
    return undefined;
  }
  const found: unknown = document._package;
  if (typeof found !== 'object' || found === null) return undefined;
  return 'get' in found &&
    'load' in found &&
    typeof found.get === 'function' &&
    typeof found.load === 'function'
    ? (found as Package)
    : undefined;
}

/** The kinds of output an unpacked part is kept as: ones nothing changes after. */
const KEPT = new Set(['string', 'blob']);

/**
 * Makes the package unpack each part once for each kind of output, however
 * often the document refers to it. A part is known by the archive's own
 * entry for it, so two spellings of one path are one part.
 */
function unpackOnce(pack: Package): void {
  const load = pack.load.bind(pack);
  const unpacked = new Map<unknown, Map<string, Promise<unknown>>>();
  pack.load = (path, type = 'string') => {
    const entry = pack.get(path);
    if (entry === undefined || !KEPT.has(type)) return load(path, type);
    const kinds = unpacked.get(entry) ?? new Map<string, Promise<unknown>>();
    unpacked.set(entry, kinds);
    const known = kinds.get(type);
    if (known) return known;
    const loading = load(path, type);
    kinds.set(type, loading);
    return loading;
  };
}

export const render: Render = async (bytes, place, report) => {
  let parsed: unknown;
  try {
    parsed = await parseAsync(bytes, OPTIONS);
  } catch {
    throw new UnreadableFile();
  }
  const pack = packageOf(parsed);
  // A version of the library that keeps its package otherwise fails here (as the renderer's
  // failure) rather than draws without the guard.
  if (!pack) throw new Error('docx-preview has no package to guard');
  unpackOnce(pack);
  let nodes: Node[];
  try {
    nodes = await renderDocument(parsed, OPTIONS);
  } catch {
    throw new UnreadableFile();
  }
  const styles = document.createElement('div');
  const body = document.createElement('div');
  body.className = 'docx-body';
  for (const node of nodes) (node.nodeName === 'STYLE' ? styles : body).appendChild(node);
  place.replaceChildren(styles, body);
  fallBack(styles, body);
  const view = new DocxView(body, report);
  report.shown(Math.max(1, view.pages.length));
  return view;
};
