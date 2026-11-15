/**
 * Moving a page in its wiki's tree, as the reader asks for it: the place a
 * page is at (to go back to), the place a step of the keyboard takes it to
 * (up, down, in and out), and the tree as it will be once the server takes
 * the move (shown at once).
 *
 * A place is said as the server takes it (`movePage`): below `parentId`
 * (empty for the top), right after `after` (empty for the first place,
 * left out for the last).
 */
import { orderBetween } from './page-order.ts';
import { comparePlaces, type PlacedPage } from './page-tree.ts';

/** Where a page goes, as `movePage` takes it. */
export interface PagePlace {
  parentId: string;
  after?: string;
}

/** A step of the keyboard: among the siblings, into the page above, out to the parent's level. */
export type MoveStep = 'up' | 'down' | 'in' | 'out';

/** The pages below `parentId`, in their order. */
function siblingsOf<P extends PlacedPage>(pages: readonly P[], parentId: string): P[] {
  return pages.filter((page) => page.parentId === parentId).sort(comparePlaces);
}

/** Where the page is now: below its parent, right after the sibling before it. */
export function placeOf(pages: readonly PlacedPage[], pageId: string): PagePlace | undefined {
  const page = pages.find((one) => one.id === pageId);
  if (!page) return undefined;
  const siblings = siblingsOf(pages, page.parentId);
  const at = siblings.findIndex((one) => one.id === pageId);
  return { parentId: page.parentId, after: at > 0 ? (siblings[at - 1]?.id ?? '') : '' };
}

/** Where a step of the keyboard takes the page; undefined where it goes nowhere. */
export function stepped(
  pages: readonly PlacedPage[],
  pageId: string,
  step: MoveStep,
): PagePlace | undefined {
  const page = pages.find((one) => one.id === pageId);
  if (!page) return undefined;
  const siblings = siblingsOf(pages, page.parentId);
  const at = siblings.findIndex((one) => one.id === pageId);
  const parentId = page.parentId;
  switch (step) {
    case 'up':
      if (at < 1) return undefined;
      return { parentId, after: at > 1 ? (siblings[at - 2]?.id ?? '') : '' };
    case 'down': {
      const next = siblings[at + 1];
      return next ? { parentId, after: next.id } : undefined;
    }
    case 'in': {
      const above = siblings[at - 1];
      return above ? { parentId: above.id } : undefined;
    }
    case 'out':
      if (parentId === '') return undefined;
      return {
        parentId: pages.find((one) => one.id === parentId)?.parentId ?? '',
        after: parentId,
      };
    default:
      return undefined;
  }
}

/** Whether the page is below `to.parentId` at the place `to.after` says. */
export function isPlaced(pages: readonly PlacedPage[], pageId: string, to: PagePlace): boolean {
  const now = placeOf(pages, pageId);
  if (now?.parentId !== to.parentId) return false;
  if (to.after !== undefined) return now.after === to.after;
  return siblingsOf(pages, to.parentId).at(-1)?.id === pageId;
}

/** Whether the move changes where the page is. */
export function movesAnywhere(
  pages: readonly PlacedPage[],
  pageId: string,
  to: PagePlace,
): boolean {
  const now = placeOf(pages, pageId);
  if (now?.parentId !== to.parentId) return true;
  const siblings = siblingsOf(pages, to.parentId).filter((one) => one.id !== pageId);
  const last = siblings.at(-1)?.id ?? '';
  return (to.after ?? last) !== now.after;
}

/**
 * The pages as they will be once the page is moved: below its new parent,
 * at an order between its new neighbours. Where they have no order between
 * them the server numbers them anew, and tells; meanwhile it goes last.
 */
export function moved<P extends PlacedPage>(
  pages: readonly P[],
  pageId: string,
  to: PagePlace,
): P[] {
  const siblings = siblingsOf(pages, to.parentId).filter((one) => one.id !== pageId);
  let at = siblings.length;
  if (to.after === '') at = 0;
  else if (to.after !== undefined) at = siblings.findIndex((one) => one.id === to.after) + 1;
  let order: string;
  try {
    order = orderBetween(siblings[at - 1]?.order, siblings[at]?.order);
  } catch {
    order = '~';
  }
  return pages.map((page) =>
    page.id === pageId ? { ...page, parentId: to.parentId, order } : page,
  );
}
