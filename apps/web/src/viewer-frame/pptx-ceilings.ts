/**
 * The ceilings on what a presentation may ask of the frame, checked in the
 * PowerPoint worker before the frame gets anything (`pptx-read.ts`).
 *
 * The renderer draws with the frame's own layout: it measures text to fit
 * it to its box, so it cannot draw in a worker. The frame, whose thread the
 * app shares, draws one slide at a time and gives the thread back between
 * them (`pptx-view.ts`). So what has to be bounded is the work of one slide
 * and the work done before any is drawn:
 *
 * - before the first slide, the renderer parses the presentation, its
 *   themes, slide masters and layouts and every relationship part at once
 *   (`EAGER_ELEMENT_LIMIT` holds them together);
 * - a slide is drawn with its layout's and its master's shapes and the
 *   SmartArt drawings it shows, so each ceiling of a slide holds for the
 *   slide, its layout, its master and its drawings together;
 * - a picture is decoded to its pixels, and one the slide recolours is
 *   worked out pixel by pixel on the frame's thread;
 * - the text is laid out once for each tab in it;
 * - a place or size beyond any slide's (`EXTENT_LIMIT`), or a slide size
 *   beyond PowerPoint's, is a lie of the file.
 *
 * Past any of them the file is refused as too large (`TooMuch`), before the
 * frame draws anything: never once a slide scrolls into view.
 */
import type { Pixels } from './pptx-images.ts';
import type { Relationship, Sorted } from './pptx-parts.ts';
import type { PartMeasure } from './pptx-xml.ts';
import { TooMuch } from './worker-xml.ts';

/** The most slides: several times a long deck's. */
export const SLIDE_LIMIT = 1_000;
/** The most XML elements the parts parsed before the first slide may have together. */
export const EAGER_ELEMENT_LIMIT = 300_000;
/** The largest slide side PowerPoint allows (56 inches), in EMU. */
export const SLIDE_SIDE_LIMIT = 51_206_400;
/** The farthest place or largest size a shape may give, in EMU: ten times the largest slide's side. */
export const EXTENT_LIMIT = 10 * SLIDE_SIDE_LIMIT;
/** The most pixels one picture may decode to (a 48-megapixel photograph has fewer). */
export const PICTURE_PIXEL_LIMIT = 50_000_000;

/**
 * The ceilings of one slide, with its layout, its master and its drawings.
 * The renderer took about a quarter of a second to draw a slide at any one
 * of `elements`, `shapes`, `textBodies`, `cells` or `text` (Chromium, on a
 * machine with other work); the time grows faster than the count past them
 * (2,000 text boxes took half a minute).
 */
export const SLIDE_LIMITS = {
  elements: 40_000,
  depth: 64,
  text: 100_000,
  tabs: 1_000,
  shapes: 1_000,
  /** Text boxes and shapes with text, which the renderer measures one by one. */
  textBodies: 250,
  groups: 16,
  cells: 1_000,
  pictures: 300,
  /** The pixels of the pictures it shows, each time it shows one. */
  pixels: 150_000_000,
  /** The pixels of the pictures it recolours. */
  recoloured: 4_000_000,
  /**
   * The share of its ceiling each of the five above takes, summed: a slide
   * near several of them at once takes as long as several slides.
   */
  work: 2,
} as const;

/** What the drawing time of a slide grows with: the ceilings that `work` sums the shares of. */
const WORK = ['elements', 'shapes', 'textBodies', 'cells', 'text'] as const;

type SlideCount = Record<keyof typeof SLIDE_LIMITS, number>;

const LAYOUT = '/slideLayout';
const MASTER = '/slideMaster';

/** The part a relationship of `from` leads to, as the renderer finds it. */
export function resolve(from: string, target: string): string {
  const base = target.startsWith('/') ? [] : from.split('/').slice(0, -1);
  for (const segment of target.replace(/^\/+/, '').split('/')) {
    if (segment === '..') base.pop();
    else if (segment !== '.' && segment !== '') base.push(segment);
  }
  return base.join('/');
}

/** The name of a part's relationship part. */
const relationshipsOf = (part: string): string => {
  const at = part.lastIndexOf('/');
  return `${part.slice(0, at)}/_rels/${part.slice(at + 1)}.rels`;
};

/** The picture a relationship's target names, as the renderer looks it up (under `ppt/media/`). */
function mediaOf(target: string): string {
  const segments = target.split(/[?#]/)[0]?.replace(/\\/g, '/').split('/') ?? [];
  const at = segments.lastIndexOf('media');
  const rest = at >= 0 && at < segments.length - 1 ? segments.slice(at + 1) : segments.slice(-1);
  return `ppt/media/${rest.join('/')}`;
}

function over(what: string): never {
  throw new TooMuch(what);
}

class SlideCounter {
  readonly #sorted: Sorted;
  readonly count: SlideCount = {
    elements: 0,
    depth: 0,
    text: 0,
    tabs: 0,
    shapes: 0,
    textBodies: 0,
    groups: 0,
    cells: 0,
    pictures: 0,
    pixels: 0,
    recoloured: 0,
    work: 0,
  };

  constructor(sorted: Sorted) {
    this.#sorted = sorted;
  }

  /** Adds a part, its pictures found through `relationships` (its own, or the slide's for a drawing). */
  add(part: PartMeasure, relationships: readonly Relationship[], from: string): void {
    const { count } = this;
    count.elements += part.elements;
    count.depth = Math.max(count.depth, part.depth);
    count.text += part.text;
    count.tabs += part.tabs;
    count.shapes += part.shapes;
    count.textBodies += part.textBodies;
    count.groups = Math.max(count.groups, part.groups);
    count.cells += part.cells;
    count.pictures += part.pictures.length;
    for (const shown of part.pictures) {
      const target = relationships.find((each) => each.id === shown.embed)?.target;
      const pixels: Pixels | undefined =
        target === undefined ? undefined : this.#sorted.pixels.get(mediaOf(resolve(from, target)));
      const area = pixels ? pixels.width * pixels.height : 0;
      count.pixels += area;
      if (shown.recoloured) count.recoloured += area;
    }
  }

  check(): void {
    const { count } = this;
    count.work = WORK.reduce((sum, name) => sum + count[name] / SLIDE_LIMITS[name], 0);
    for (const [name, limit] of Object.entries(SLIDE_LIMITS)) {
      if (this.count[name as keyof SlideCount] > limit) over(name);
    }
  }
}

/** The part a part's relationship of `type` leads to, if it is one the renderer has. */
function related(sorted: Sorted, part: string, type: string): string | undefined {
  const found = sorted.relationships
    .get(relationshipsOf(part))
    ?.find((each) => each.type.endsWith(type));
  return found ? resolve(part, found.target) : undefined;
}

/** What drawing slide `slide` asks for; throws `TooMuch` past a ceiling of a slide. */
function checkSlide(sorted: Sorted, slide: string): Set<string> {
  const counter = new SlideCounter(sorted);
  const own = sorted.relationships.get(relationshipsOf(slide)) ?? [];
  const layout = related(sorted, slide, LAYOUT);
  const master = layout === undefined ? undefined : related(sorted, layout, MASTER);
  const characters = new Set<string>();
  const take = (
    part: string | undefined,
    relationships: readonly Relationship[],
    from: string,
  ): void => {
    const measured = part === undefined ? undefined : sorted.measures.get(part);
    if (!measured) return;
    counter.add(measured, relationships, from);
    for (const character of measured.characters) characters.add(character);
  };
  take(slide, own, slide);
  for (const part of [layout, master]) {
    if (part !== undefined) take(part, sorted.relationships.get(relationshipsOf(part)) ?? [], part);
  }
  // A SmartArt drawing's pictures are found through the slide's relationships.
  for (const { target } of own) {
    const part = resolve(slide, target);
    if (sorted.parts.diagramDrawings.has(part)) take(part, own, slide);
  }
  counter.check();
  return characters;
}

/** The parts the renderer parses before it draws any slide. */
function eagerParts(sorted: Sorted): string[] {
  const { parts } = sorted;
  return [
    '[Content_Types].xml',
    'ppt/presentation.xml',
    'ppt/_rels/presentation.xml.rels',
    'ppt/tableStyles.xml',
    ...[parts.themes, parts.slideMasters, parts.slideLayouts].flatMap((map) => [...map.keys()]),
    ...sorted.relationships.keys(),
  ];
}

/**
 * Checks the presentation against every ceiling; throws `TooMuch` past one.
 * Returns the characters of all the slides' text, each once.
 */
export function checkCeilings(sorted: Sorted): string {
  const { measures, parts } = sorted;
  for (const measured of measures.values()) {
    if (measured.extent > EXTENT_LIMIT) over('extent');
  }
  for (const pixels of sorted.pixels.values()) {
    if (pixels.width * pixels.height > PICTURE_PIXEL_LIMIT) over('picture');
  }
  const presentation = measures.get('ppt/presentation.xml');
  const size = presentation?.size;
  if (size && (size.cx > SLIDE_SIDE_LIMIT || size.cy > SLIDE_SIDE_LIMIT)) over('slide size');
  let eager = 0;
  for (const part of new Set(eagerParts(sorted))) eager += measures.get(part)?.elements ?? 0;
  if (eager > EAGER_ELEMENT_LIMIT) over('elements before the first slide');
  // The renderer takes the slides of the list, or every slide relationship when the list has none.
  const listed = presentation?.slideIds ?? 0;
  const slides = (sorted.relationships.get('ppt/_rels/presentation.xml.rels') ?? []).filter(
    (each) => each.type.endsWith('/slide'),
  );
  if (Math.max(listed, slides.length, parts.slides.size) > SLIDE_LIMIT) over('slides');
  const characters = new Set<string>();
  for (const slide of parts.slides.keys()) {
    for (const character of checkSlide(sorted, slide)) characters.add(character);
  }
  return [...characters].join('');
}
