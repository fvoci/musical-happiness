import { describe, expect, it } from 'vitest';

import { allPages, firstOf, MAX_PER_PAGE } from '../../src/lib/state/page-reads.ts';
import type { Paging } from '../../src/lib/state/trash-reads.ts';

/** A list of `length` numbers read a page at a time, and the pages asked for. */
function list(length: number): { read: (paging: Paging) => Promise<number[]>; asked: Paging[] } {
  const items = Array.from({ length }, (_, index) => index);
  const asked: Paging[] = [];
  const read = (paging: Paging): Promise<number[]> => {
    asked.push(paging);
    const from = paging.page * paging.perPage;
    return Promise.resolve(items.slice(from, from + paging.perPage));
  };
  return { read, asked };
}

describe('reading a list in pages', () => {
  it('reads every item of a list of more than a page, a page of 1000 at a time', async () => {
    const { read, asked } = list(2 * MAX_PER_PAGE + 1);
    const items = await allPages(read);
    expect(items).toHaveLength(2001);
    expect(items.at(-1)).toBe(2000);
    expect(asked).toEqual([
      { page: 0, perPage: 1000 },
      { page: 1, perPage: 1000 },
      { page: 2, perPage: 1000 },
    ]);
  });

  it('asks once more when the last page is full, and stops at the empty one', async () => {
    const { read, asked } = list(MAX_PER_PAGE);
    expect(await allPages(read)).toHaveLength(1000);
    expect(asked).toEqual([
      { page: 0, perPage: 1000 },
      { page: 1, perPage: 1000 },
    ]);
  });

  it('reads the first items in one page when they fit, and in pages of 1000 when not', async () => {
    const small = list(80);
    expect(await firstOf(small.read, 50)).toHaveLength(50);
    expect(small.asked).toEqual([{ page: 0, perPage: 50 }]);
    const large = list(1500);
    const items = await firstOf(large.read, 1200);
    expect(items).toEqual(Array.from({ length: 1200 }, (_, index) => index));
    expect(large.asked).toEqual([
      { page: 0, perPage: 1000 },
      { page: 1, perPage: 1000 },
    ]);
    const none = list(5);
    expect(await firstOf(none.read, 0)).toEqual([]);
    expect(none.asked).toEqual([]);
  });
});
