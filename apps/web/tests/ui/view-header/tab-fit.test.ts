/** Which tabs of the views fit in a row, each at its whole width. */
import { describe, expect, it } from 'vitest';

import { fittingTabs } from '$lib/features/view-header/tab-fit.ts';

describe('fittingTabs', () => {
  it('shows every tab where all of them fit, the button of the rest not counted', () => {
    expect(fittingTabs([100, 100, 100], 300, 80, 0)).toEqual([0, 1, 2]);
  });

  it('shows every tab while nothing is measured', () => {
    expect(fittingTabs([0, 0, 0], 0, 0, 2)).toEqual([0, 1, 2]);
    expect(fittingTabs([100, 100], 0, 80, 1)).toEqual([0, 1]);
  });

  it('keeps as many as fit beside the button of the rest, in their order', () => {
    expect(fittingTabs([100, 100, 100, 100], 300, 80, 0)).toEqual([0, 1]);
  });

  it('puts the current tab in place of the last that fits where the room ends before it', () => {
    expect(fittingTabs([100, 100, 100, 100], 300, 80, 3)).toEqual([0, 3]);
    // A wide current tab takes the place of more than one.
    expect(fittingTabs([100, 100, 100, 190], 400, 80, 3)).toEqual([0, 3]);
  });

  it('shows the current tab alone where even it does not fit', () => {
    expect(fittingTabs([100, 400, 100], 300, 80, 1)).toEqual([1]);
  });
});
