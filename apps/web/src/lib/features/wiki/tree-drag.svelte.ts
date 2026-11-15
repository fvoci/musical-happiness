/**
 * Dragging a page of the wiki's tree to another place: between two pages,
 * or onto a page, to go below it. A mouse takes the page once it has moved a
 * few pixels with the button down (a click still opens it); a finger rests
 * on it first, and one that moves at once scrolls (`pointerDrag`).
 *
 * The row under the pointer says where: its upper quarter puts the page
 * before it, its lower quarter after it (first below it, when its pages
 * show), and the rest below it, last. A folded page held over long enough
 * unfolds. A page goes nowhere below itself: over the page dragged, or a
 * page below it, nothing is marked and letting go changes nothing. Escape
 * cancels.
 */
import { pointerDrag, type PointerDragSession } from '$lib/drag/pointer-drag.ts';
import type { Point } from '$lib/logic/drop-target.ts';
import type { PagePlace } from '$lib/logic/page-moves.ts';
import { comparePlaces, withDescendants, type PageRow } from '$lib/logic/page-tree.ts';
import type { WikiPage } from '$lib/state/index.ts';

/** How long a folded page is held over before it unfolds, in milliseconds. */
const UNFOLD_AFTER = 700;

/** Where a page let go would go: the row it is over, and how. */
export interface DropMark {
  /** The page whose row is under the pointer. */
  pageId: string;
  zone: 'before' | 'after' | 'inside';
  /** How deep the page would be: 0 at the top. */
  depth: number;
  to: PagePlace;
}

export interface TreeDragSource {
  rows: () => readonly PageRow<WikiPage>[];
  pages: () => readonly WikiPage[];
  /** Pages may be dragged: by an editor. */
  enabled: () => boolean;
  /** Unfolds a page held over. */
  unfold: (pageId: string) => void;
  ondrop: (pageId: string, to: PagePlace) => void;
}

/** The page whose row is under a point, within the tree. */
function rowAt(root: HTMLElement, point: Point): HTMLElement | undefined {
  const hit = document.elementFromPoint(point.x, point.y);
  const row = hit?.closest<HTMLElement>('[data-page]');
  return row && root.contains(row) ? row : undefined;
}

/** The place before `row`'s page among its siblings, the page dragged left out. */
function before(pages: readonly WikiPage[], row: PageRow<WikiPage>, dragged: string): PagePlace {
  const parentId = row.parentId ?? '';
  const siblings = pages
    .filter((page) => page.parentId === parentId && page.id !== dragged)
    .sort(comparePlaces);
  const at = siblings.findIndex((page) => page.id === row.page.id);
  return { parentId, after: at > 0 ? (siblings[at - 1]?.id ?? '') : '' };
}

export class TreeDrag {
  readonly #source: TreeDragSource;
  /** The page being dragged. */
  dragged = $state<string | undefined>(undefined);
  /** Where it would go if let go now; none where it can go nowhere. */
  mark = $state.raw<DropMark | undefined>(undefined);
  /** Where the pointer is while a page is dragged: its title follows it. */
  at = $state.raw<Point | undefined>(undefined);
  #root: HTMLElement | undefined;
  #unfolding: { pageId: string; timer: ReturnType<typeof setTimeout> } | undefined;

  constructor(source: TreeDragSource) {
    this.#source = source;
  }

  /** The list of the tree. For `{@attach}`. */
  readonly attach = (root: HTMLElement): (() => void) => {
    this.#root = root;
    const handle = pointerDrag(root, this.#press);
    return () => {
      handle.destroy();
      this.#stopUnfolding();
      this.#root = undefined;
    };
  };

  #stopUnfolding(): void {
    if (this.#unfolding) clearTimeout(this.#unfolding.timer);
    this.#unfolding = undefined;
  }

  /** Unfolds the folded page held over, once it has been held long enough. */
  #holdOver(row: PageRow<WikiPage> | undefined): void {
    const pageId = row && row.hasChildren && !row.open ? row.page.id : undefined;
    if (this.#unfolding?.pageId === pageId) return;
    this.#stopUnfolding();
    if (pageId === undefined) return;
    const timer = setTimeout(() => {
      this.#unfolding = undefined;
      this.#source.unfold(pageId);
    }, UNFOLD_AFTER);
    this.#unfolding = { pageId, timer };
  }

  /** Where the page dragged would go, over the point. */
  #markAt(dragged: string, point: Point): DropMark | undefined {
    const root = this.#root;
    const element = root && rowAt(root, point);
    const row = this.#source.rows().find((one) => one.page.id === element?.dataset.page);
    if (!element || !row) {
      this.#holdOver(undefined);
      return undefined;
    }
    const pages = this.#source.pages();
    const box = element.getBoundingClientRect();
    const share = box.height > 0 ? (point.y - box.top) / box.height : 0.5;
    const zone = share < 0.25 ? 'before' : share > 0.75 ? 'after' : 'inside';
    this.#holdOver(zone === 'inside' ? row : undefined);
    // Nowhere below itself.
    if (withDescendants(pages, dragged).has(row.page.id)) return undefined;
    const parentId = row.parentId ?? '';
    if (zone === 'before') return { ...this.#mark(row, zone), to: before(pages, row, dragged) };
    if (zone === 'inside') {
      return { ...this.#mark(row, zone), depth: row.depth + 1, to: { parentId: row.page.id } };
    }
    // After a page whose pages show: first among them.
    if (row.open) {
      return {
        ...this.#mark(row, zone),
        depth: row.depth + 1,
        to: { parentId: row.page.id, after: '' },
      };
    }
    return { ...this.#mark(row, zone), to: { parentId, after: row.page.id } };
  }

  #mark(row: PageRow<WikiPage>, zone: DropMark['zone']): Omit<DropMark, 'to'> {
    return { pageId: row.page.id, zone, depth: row.depth };
  }

  readonly #press = (event: PointerEvent): PointerDragSession | undefined => {
    if (!this.#source.enabled()) return undefined;
    const target = event.target instanceof Element ? event.target : null;
    // The buttons of a row (its fold, its menu) keep their presses.
    if (!target || target.closest('button')) return undefined;
    const pageId = target.closest<HTMLElement>('[data-page]')?.dataset.page;
    if (pageId === undefined) return undefined;
    const show = (point: Point): void => {
      this.at = point;
      this.mark = this.#markAt(pageId, point);
    };
    const end = (): void => {
      this.#stopUnfolding();
      [this.dragged, this.mark, this.at] = [undefined, undefined, undefined];
    };
    return {
      start: (point) => {
        this.dragged = pageId;
        show(point);
      },
      move: show,
      drop: (point) => {
        show(point);
        const to = this.mark?.to;
        end();
        if (to) this.#source.ondrop(pageId, to);
      },
      cancel: end,
    };
  };
}
