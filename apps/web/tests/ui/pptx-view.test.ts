/**
 * The slides the frame draws at once (src/viewer-frame/pptx-view.ts): no
 * more pixels of pictures together than one slide may show, and a slide in
 * view drawn all the same, in place of slides drawn out of view.
 */
import type { PresentationData, SlideHandle } from '@aiden0z/pptx-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { PIXELS_DRAWN, PptxView } from '../../src/viewer-frame/pptx-view.ts';

/** Where each slide's box is in the frame's view, by its page: [top, bottom]. */
let places = new Map<number, [number, number]>();
let observer: { callback: IntersectionObserverCallback } | undefined;

class FakeObserver {
  constructor(callback: IntersectionObserverCallback) {
    observer = { callback };
  }
  /** Every box is observed; `sight` says which are near the view. */
  observe(): void {
    return undefined;
  }
  disconnect(): void {
    return undefined;
  }
}

/** Tells the view which boxes are near the view now (all with a place). */
function sight(boxes: Element[]): void {
  const entries = boxes.map((box) => {
    const page = Number((box as HTMLElement).dataset.page);
    return {
      target: box,
      isIntersecting: places.has(page),
    } as unknown as IntersectionObserverEntry;
  });
  observer?.callback(entries, {} as IntersectionObserver);
}

beforeEach(() => {
  places = new Map();
  vi.stubGlobal('IntersectionObserver', FakeObserver);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    const [top, bottom] = places.get(Number(this.dataset.page)) ?? [-10_000, -9_000];
    return { top, bottom, left: 0, right: 100, width: 100, height: bottom - top } as DOMRect;
  });
  vi.stubGlobal('innerHeight', 900);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** A view of `pixels.length` slides, each showing that many pixels; and the pages drawn. */
function view(pixels: number[]): { view: PptxView; drawn: () => number[] } {
  const presentation = {
    width: 960,
    height: 540,
    slides: pixels.map((_, index) => ({ slidePath: `ppt/slides/slide${index + 1}.xml` })),
  } as unknown as PresentationData;
  const live = new Set<number>();
  const made = new Map(pixels.map((each, index) => [`ppt/slides/slide${index + 1}.xml`, each]));
  const place = document.createElement('div');
  const shown = new PptxView(
    presentation,
    place,
    { shown: () => undefined, page: () => undefined },
    (index): SlideHandle => {
      live.add(index + 1);
      return {
        element: document.createElement('div'),
        ready: Promise.resolve(),
        dispose: () => live.delete(index + 1),
      } as unknown as SlideHandle;
    },
    'chart',
    made,
  );
  return { view: shown, drawn: () => [...live].sort((a, b) => a - b) };
}

/** Lets the view draw what it will. */
const settle = async (): Promise<void> => {
  await vi.advanceTimersByTimeAsync(2_000);
};

describe('the slides drawn at once', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('show no more pixels together than one slide may: one only near the view waits', async () => {
    const share = PIXELS_DRAWN * 0.4;
    const { view: shown, drawn } = view([share, share, share]);
    places = new Map([
      [1, [0, 540]],
      [2, [560, 1100]],
      [3, [1120, 1660]],
    ]);
    sight(shown.boxes);
    await settle();
    expect(drawn()).toEqual([1, 2]);
  });

  it('a slide in view is drawn in place of slides drawn out of view', async () => {
    const most = PIXELS_DRAWN - 1;
    const { view: shown, drawn } = view([most, most, most]);
    places = new Map([[1, [0, 540]]]);
    sight(shown.boxes);
    await settle();
    expect(drawn()).toEqual([1]);
    // Scrolled on: slide 1 is above the view but near it, slide 2 in view.
    places = new Map([
      [1, [-600, -60]],
      [2, [0, 540]],
    ]);
    sight(shown.boxes);
    await settle();
    expect(drawn()).toEqual([2]);
  });
});
