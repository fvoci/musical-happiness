/**
 * The place a card was scrolled to, kept while its box changes size. The
 * simulated page has no layout, so only the two decisions are tested here;
 * the scrolling itself is tested in a browser (e2e/card.spec.ts, "the window
 * changes size under an open card").
 */
import { describe, expect, it } from 'vitest';

import { afterResize, afterScroll, type Box, type Kept } from '$lib/features/card/scroll-keeper.ts';

const PHONE = { width: 360, height: 600 };
const WIDE = { width: 768, height: 560 };

const kept = (top: number, size = PHONE): Kept => ({ top, ...size });
const box = (top: number, max: number, size = PHONE): Box => ({ top, max, ...size });

describe('after a scroll event', () => {
  it("remembers the reader's own scrolling", () => {
    expect(afterScroll(kept(0), box(240, 900))).toEqual(kept(240));
    expect(afterScroll(kept(240), box(10, 900))).toEqual(kept(10));
  });

  it('does not take a position the browser cut down in a resized box for the reader', () => {
    // 400 down on a phone; in the wide dialog there are only 50 pixels to scroll.
    expect(afterScroll(kept(400), box(50, 50, WIDE))).toEqual(kept(400));
    // Positions are fractions on some screens: nearly at the end is at the end.
    expect(afterScroll(kept(400), box(49.5, 50, WIDE))).toEqual(kept(400));
  });

  it('remembers scrolling in the resized box that is not at its end', () => {
    expect(afterScroll(kept(400), box(20, 50, WIDE))).toEqual(kept(20, WIDE));
  });

  it('remembers scrolling further down in the resized box', () => {
    expect(afterScroll(kept(100), box(300, 300, WIDE))).toEqual(kept(300, WIDE));
  });

  it('takes the end of a box that did not change size for what it is', () => {
    // The content got shorter (a comment was deleted): that is where the reader is now.
    expect(afterScroll(kept(400), box(350, 350))).toEqual(kept(350));
  });
});

describe('after the box changed size', () => {
  it('goes back to the remembered place when there is room for it again', () => {
    expect(afterResize(kept(400), box(50, 900))).toBe(400);
  });

  it('goes as near as the box can scroll', () => {
    expect(afterResize(kept(400), box(50, 120, WIDE))).toBe(120);
  });

  it('leaves the position alone when it is already there or cannot get nearer', () => {
    expect(afterResize(kept(400), box(400, 900, WIDE))).toBeUndefined();
    expect(afterResize(kept(400), box(50, 50, WIDE))).toBeUndefined();
    expect(afterResize(kept(0), box(0, 0, WIDE))).toBeUndefined();
  });

  it('never scrolls up: a position further down was the reader moving on', () => {
    expect(afterResize(kept(100), box(300, 900, WIDE))).toBeUndefined();
  });

  it('gives the place back after a round of sizes', () => {
    // Phone, 120 down. Wide: cut to 50. Phone again: back to 120.
    let memory = kept(120);
    memory = afterScroll(memory, box(50, 50, WIDE));
    expect(afterResize(memory, box(50, 50, WIDE))).toBeUndefined();
    expect(afterResize(memory, box(50, 700))).toBe(120);
    memory = afterScroll(memory, box(120, 700));
    expect(memory).toEqual(kept(120));
  });
});
