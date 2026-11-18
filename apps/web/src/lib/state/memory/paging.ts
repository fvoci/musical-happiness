/**
 * The pages of a list, cut as the server cuts them: `page` and `perPage` are
 * whole numbers from 0 to 2147483647 (anything else is 400 at the parameter),
 * and each list has its default `perPage` and its own reading of 0.
 */
import { BackendError, problemOf } from '../backend.ts';
import type { Paging } from '../trash-reads.ts';

/** The largest `page` or `perPage` the server takes. */
const MAX_COUNT = 2_147_483_647;

/** How a list reads `perPage`. */
export interface PageRule {
  /** The `perPage` of a request without one. */
  defaultPerPage: number;
  /** What `perPage` 0 is: every item, or the default. */
  zero: 'all' | 'default';
  /** The most a page holds; more asked is this many. */
  cap?: number;
}

/** Cards, trash and the pages of a wiki: 100 to a page, 0 for every item. */
export const CARD_PAGES: PageRule = { defaultPerPage: 100, zero: 'all' };

/** What names a block: 50 to a page, 0 for the default, at most 200. */
export const REFERENCE_PAGES: PageRule = { defaultPerPage: 50, zero: 'default', cap: 200 };

function checked(parameter: string, count: number): number {
  if (Number.isInteger(count) && count >= 0 && count <= MAX_COUNT) return count;
  const detail = `invalid \`${parameter}\` parameter: ${count < 0 ? 'below 0' : 'out of range'}`;
  throw new BackendError(400, {
    ...problemOf(400, detail),
    errors: [{ parameter, detail, reason: 'invalid' }],
  });
}

/** The page `paging` asks for of `items`, by `rule`. */
export function pageOf<T>(items: readonly T[], paging: Paging | undefined, rule: PageRule): T[] {
  const page = checked('page', paging?.page ?? 0);
  const asked = checked('perPage', paging?.perPage ?? rule.defaultPerPage);
  if (asked === 0 && rule.zero === 'all') return [...items];
  const perPage = Math.min(asked === 0 ? rule.defaultPerPage : asked, rule.cap ?? Infinity);
  return items.slice(page * perPage, (page + 1) * perPage);
}
