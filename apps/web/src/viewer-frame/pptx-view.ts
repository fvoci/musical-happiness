/**
 * The slides of a presentation in the frame: a box of each slide's size at
 * once, and in them only the slides in view (and a screen's height around
 * them), drawn one at a time.
 *
 * The renderer draws with the frame's own layout, on the frame's thread,
 * which the app shares. So a slide is drawn, cleaned (`pptx-clean.ts`) and
 * put in its box, the renderer is let fit its text again on the frames
 * after (it does so two frames on), and only then, the thread given back
 * once more, is the next slide drawn. What one slide may ask is bounded in
 * the worker (`pptx-ceilings.ts`). A slide that scrolls far away is taken
 * out of the page.
 *
 * A slide is drawn at its own size and scaled with a transform, so the
 * renderer's measuring sees the sizes it laid out; zooming scales the boxes
 * without drawing again.
 */
import type { PresentationData, SlideHandle, SlideRendererOptions } from '@aiden0z/pptx-renderer';

import { cleanSlide } from './pptx-clean.ts';
import { PageFollower, type Drawn, type Report } from './renderer.ts';

/** The room around the slides, in CSS pixels, at each side. */
const GUTTER = 16;
/**
 * The most slides drawn at once, whatever the deck's slide size and the
 * zoom put in view: the frame's memory is the app's process's.
 */
const DRAWN_LIMIT = 16;

/** Draws the slide at `index` (from 0). */
export type DrawSlide = (index: number, options: SlideRendererOptions) => SlideHandle;

const nextFrame = (): Promise<void> =>
  new Promise((resolve) => {
    requestAnimationFrame(() => {
      resolve();
    });
  });

const yieldThread = (): Promise<void> =>
  new Promise((resolve) => {
    setTimeout(resolve, 0);
  });

/** One slide: its box in the column, and the slide drawn in it while it is in view. */
class Slide {
  readonly box = document.createElement('div');
  readonly number: number;
  handle: SlideHandle | undefined;
  /** The renderer could not draw it: it stays an empty box and is not tried again. */
  failed = false;

  constructor(number: number) {
    this.number = number;
    this.box.className = 'page slide';
    this.box.dataset.page = String(number);
  }

  clear(): void {
    this.handle?.dispose();
    this.handle = undefined;
    this.box.replaceChildren();
  }
}

export class PptxView implements Drawn {
  readonly #presentation: PresentationData;
  readonly #draw: DrawSlide;
  /** What the box of a chart says. */
  readonly #chart: string;
  readonly #slides: Slide[];
  readonly #visible = new Set<Slide>();
  readonly #observer: IntersectionObserver;
  readonly #follower: PageFollower;
  readonly #options: SlideRendererOptions;
  #zoom = 1;
  #drawing = false;

  constructor(
    presentation: PresentationData,
    place: HTMLElement,
    report: Report,
    draw: DrawSlide,
    chart: string,
  ) {
    this.#presentation = presentation;
    this.#draw = draw;
    this.#chart = chart;
    this.#options = {
      externalMedia: 'block',
      pdfjs: false,
      // One address for each picture, made once, for as long as the frame lasts.
      mediaUrlCache: new Map(),
      onNavigate: ({ slideIndex }) => {
        if (slideIndex !== undefined) this.goTo(slideIndex + 1);
      },
    };
    const column = document.createElement('div');
    column.className = 'pages';
    this.#slides = presentation.slides.map((_, index) => new Slide(index + 1));
    column.append(...this.#slides.map((slide) => slide.box));
    place.replaceChildren(column);
    this.#observer = new IntersectionObserver(
      (entries) => {
        this.#onSight(entries);
      },
      // The document as the root: the frame's own viewport, whose margin is taken (it is not
      // when the root is left out, the frame being of another origin than the page).
      { root: document, rootMargin: '100% 0px' },
    );
    this.#layOut();
    for (const slide of this.#slides) this.#observer.observe(slide.box);
    this.#follower = new PageFollower(() => this.boxes, report);
    let resizing: ReturnType<typeof setTimeout> | undefined;
    window.addEventListener('resize', () => {
      clearTimeout(resizing);
      resizing = setTimeout(() => {
        this.#layOut();
      }, 150);
    });
  }

  get boxes(): Element[] {
    return this.#slides.map((slide) => slide.box);
  }

  /** The scale at the zoom: the slide as wide as the frame allows, at most its own size. */
  #scale(): number {
    const room = Math.max(1, window.innerWidth - 2 * GUTTER);
    return Math.min(1, room / this.#presentation.width) * this.#zoom;
  }

  #layOut(): void {
    const scale = this.#scale();
    const { width, height } = this.#presentation;
    for (const { box } of this.#slides) {
      box.style.width = `${Math.floor(width * scale)}px`;
      box.style.height = `${Math.floor(height * scale)}px`;
      const drawn = box.firstElementChild;
      if (drawn instanceof HTMLElement) drawn.style.transform = `scale(${scale})`;
    }
  }

  /** Draws one slide into its box and lets the renderer finish fitting it. */
  async #drawSlide(slide: Slide): Promise<void> {
    const handle = this.#draw(slide.number - 1, this.#options);
    const { element } = handle;
    cleanSlide(element, this.#chart);
    element.style.transformOrigin = '0 0';
    element.style.transform = `scale(${this.#scale()})`;
    slide.handle = handle;
    slide.box.replaceChildren(element);
    await handle.ready;
    // The renderer fits text again two frames on: that is part of drawing this slide.
    await nextFrame();
    await nextFrame();
    // What the renderer set meanwhile (its pictures' recolouring, its refitting) is cleaned too.
    if (slide.handle === handle) cleanSlide(element, this.#chart);
  }

  /**
   * The slide to draw next: of those near the view not drawn yet, one in the
   * frame's view before one only near it, the first first; none while
   * `DRAWN_LIMIT` slides are drawn.
   */
  #next(): Slide | undefined {
    const drawn = this.#slides.filter((slide) => slide.handle).length;
    if (drawn >= DRAWN_LIMIT) return undefined;
    const inView = (slide: Slide): number => {
      const { top, bottom } = slide.box.getBoundingClientRect();
      return bottom > 0 && top < window.innerHeight ? 0 : 1;
    };
    return [...this.#visible]
      .filter((slide) => !slide.handle && !slide.failed)
      .map((slide) => ({ slide, rank: inView(slide) }))
      .sort((a, b) => a.rank - b.rank || a.slide.number - b.slide.number)[0]?.slide;
  }

  /** Draws the slides in view one after another, the thread given back between them. */
  async #drawInView(): Promise<void> {
    if (this.#drawing) return;
    this.#drawing = true;
    try {
      for (let slide = this.#next(); slide; slide = this.#next()) {
        await this.#drawSlide(slide).catch(() => {
          slide.clear();
          slide.failed = true;
        });
        await yieldThread();
      }
    } finally {
      this.#drawing = false;
    }
  }

  #onSight(entries: IntersectionObserverEntry[]): void {
    for (const entry of entries) {
      const slide = this.#slides[Number((entry.target as HTMLElement).dataset.page) - 1];
      if (!slide) continue;
      if (entry.isIntersecting) this.#visible.add(slide);
      else {
        this.#visible.delete(slide);
        slide.clear();
      }
    }
    void this.#drawInView();
  }

  /** Draws the first slide, the one in view when the presentation opens; rejects when it cannot be drawn. */
  async drawFirst(): Promise<void> {
    const first = this.#slides[0];
    if (!first) return;
    // The slides in view wait for it (the observer tells of them once the column is laid out).
    this.#drawing = true;
    try {
      await this.#drawSlide(first);
    } finally {
      this.#drawing = false;
    }
    void this.#drawInView();
  }

  goTo(page: number): void {
    const box = this.#slides[page - 1]?.box;
    if (!box) return;
    box.scrollIntoView({ block: 'start' });
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
