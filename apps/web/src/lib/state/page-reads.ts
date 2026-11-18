/**
 * Reading a list the server answers in pages: every list takes `perPage`
 * from 1 to {@link MAX_PER_PAGE}, so what needs more reads page after page.
 */
import type { Paging } from './trash-reads.ts';

/** The most a page of any list holds. */
export const MAX_PER_PAGE = 1000;

/** One page of a list, as `read` asks for it. */
export type PageReader<T> = (paging: Paging) => Promise<T[]>;

/** Every item of a list, read a page of {@link MAX_PER_PAGE} at a time until a page is short. */
export function allPages<T>(read: PageReader<T>): Promise<T[]> {
  return firstOf(read, Infinity);
}

/**
 * The first `count` items of a list, or all of them if it has fewer: one
 * page if they fit in one, else pages of {@link MAX_PER_PAGE} until there are
 * enough or a page is short.
 */
export async function firstOf<T>(read: PageReader<T>, count: number): Promise<T[]> {
  if (count <= 0) return [];
  const perPage = Math.min(count, MAX_PER_PAGE);
  const items: T[] = [];
  for (let page = 0; items.length < count; page += 1) {
    const one = await read({ page, perPage });
    items.push(...one);
    if (one.length < perPage) break;
  }
  return items.length > count ? items.slice(0, count) : items;
}
