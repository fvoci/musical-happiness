/**
 * Dragging a page of the wiki's tree to another place: between two pages,
 * or onto a page, to go below it. A mouse takes the page once it has moved a
 * few pixels with the button down (a click still opens it); a finger rests
 * on it first, and one that moves at once scrolls (`pointerDrag`).
 *
 * The row under the pointer says where: its upper quarter puts the page
 * before it, its lower quarter after it (first below it, when its pages
 * show), and the rest below it, last. After the last page of a level, how
 * far left the pointer is says how many levels up the page goes, as deep as
 * the line drawn there. A folded page held over long enough unfolds, and the
 * tree scrolls with the pointer near its top or bottom edge. A page goes
 * nowhere below itself: over the page dragged, or a page below it, nothing
 * is marked and letting go changes nothing. Escape cancels.
 */
import { AutoScroller } from '$lib/drag/auto-scroll.ts';
import { pointerDrag, type PointerDragSession } from '$lib/drag/pointer-drag.ts';
import { DRAG_SETTINGS } from '$lib/drag/settings.ts';
import type { Point } from '$lib/logic/drop-target.ts';
import type { PagePlace } from '$lib/logic/page-moves.ts';
import { comparePlaces, withDescendants, type PageRow } from '$lib/logic/page-tree.ts';
import type { WikiPage } from '$lib/state/index.ts';

/** How long a folded page is held over before it unfolds, in milliseconds. */
const UNFOLD_AFTER = 700;
/** How far in a level of the tree is drawn, in pixels (`PageTree`: 1rem a level). */
const LEVEL_WIDTH = 16;

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

/**
 * The rows a page let go after `row` may go after: the row, and while each is
 * the last of its siblings, the row it is below, up to the top.
 */
function levelsAfter(
  rows: readonly PageRow<WikiPage>[],
  row: PageRow<WikiPage>,
): PageRow<WikiPage>[] {
  const levels = [row];
  let at = row;
  while (at.position === at.siblings && at.parentId !== undefined) {
    const above = rows.find((one) => one.page.id === at.parentId);
    if (!above) break;
    levels.push(above);
    at = above;
  }
  return levels;
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
  readonly #scroller = new AutoScroller();
  #frame: number | undefined;

  constructor(source: TreeDragSource) {
    this.#source = source;
  }

  /** The list of the tree, which scrolls. For `{@attach}`. */
  readonly attach = (root: HTMLElement): (() => void) => {
    this.#root = root;
    const handle = pointerDrag(root, this.#press);
    return () => {
      handle.destroy();
      this.#stop();
      this.#root = undefined;
    };
  };

  #stop(): void {
    if (this.#unfolding) clearTimeout(this.#unfolding.timer);
    this.#unfolding = undefined;
    if (this.#frame !== undefined) cancelAnimationFrame(this.#frame);
    this.#frame = undefined;
    this.#scroller.clear();
  }

  /** Unfolds the folded page held over, once it has been held long enough. */
  #holdOver(row: PageRow<WikiPage> | undefined): void {
    const pageId = row && row.hasChildren && !row.open ? row.page.id : undefined;
    if (this.#unfolding?.pageId === pageId) return;
    if (this.#unfolding) clearTimeout(this.#unfolding.timer);
    this.#unfolding = undefined;
    if (pageId === undefined) return;
    const timer = setTimeout(() => {
      this.#unfolding = undefined;
      this.#source.unfold(pageId);
    }, UNFOLD_AFTER);
    this.#unfolding = { pageId, timer };
  }

  /** Where the page dragged would go, over the point; `moving` is it and the pages below it. */
  #markAt(dragged: string, moving: ReadonlySet<string>, point: Point): DropMark | undefined {
    const root = this.#root;
    const element = root && rowAt(root, point);
    const rows = this.#source.rows();
    const row = rows.find((one) => one.page.id === element?.dataset.page);
    // Nowhere below itself; and not beside a page drawn at the top for want of its parent.
    if (!element || !row || moving.has(row.page.id) || row.page.parentId !== (row.parentId ?? '')) {
      this.#holdOver(undefined);
      return undefined;
    }
    const box = element.getBoundingClientRect();
    const share = box.height > 0 ? (point.y - box.top) / box.height : 0.5;
    const zone = share < 0.25 ? 'before' : share > 0.75 ? 'after' : 'inside';
    this.#holdOver(zone === 'inside' ? row : undefined);
    const mark = { pageId: row.page.id, zone } as const;
    if (zone === 'before') {
      return { ...mark, depth: row.depth, to: before(this.#source.pages(), row, dragged) };
    }
    if (zone === 'inside') return { ...mark, depth: row.depth + 1, to: { parentId: row.page.id } };
    // After a page whose pages show: first among them.
    if (row.open)
      return { ...mark, depth: row.depth + 1, to: { parentId: row.page.id, after: '' } };
    // After the last of a level: as many levels up as the pointer is to the left.
    const levels = levelsAfter(rows, row);
    const level = Math.floor((point.x - box.left - 4) / LEVEL_WIDTH);
    const up = Math.min(levels.length - 1, Math.max(0, row.depth - level));
    const target = levels[up] ?? row;
    return {
      ...mark,
      depth: target.depth,
      to: { parentId: target.parentId ?? '', after: target.page.id },
    };
  }

  /** Scrolls the tree with the pointer near an edge, a frame at a time, while a page is dragged. */
  #scroll(dragged: string, moving: ReadonlySet<string>): void {
    let last = performance.now();
    const frame = (now: number): void => {
      const root = this.#root;
      const at = this.at;
      if (!root || !at || this.dragged !== dragged) return;
      const elapsed = Math.min(now - last, DRAG_SETTINGS.longestFrame);
      last = now;
      if (this.#scroller.step(at, [{ element: root, axis: 'y' }], elapsed)) {
        this.mark = this.#markAt(dragged, moving, at);
      }
      this.#frame = requestAnimationFrame(frame);
    };
    this.#frame = requestAnimationFrame(frame);
  }

  readonly #press = (event: PointerEvent): PointerDragSession | undefined => {
    if (!this.#source.enabled()) return undefined;
    const target = event.target instanceof Element ? event.target : null;
    // The buttons of a row (its fold, its menu) keep their presses.
    if (!target || target.closest('button')) return undefined;
    const pageId = target.closest<HTMLElement>('[data-page]')?.dataset.page;
    if (pageId === undefined) return undefined;
    // Not shown: what the drag consults, made anew when it starts.
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    let moving: ReadonlySet<string> = new Set([pageId]);
    const show = (point: Point): void => {
      this.at = point;
      this.mark = this.#markAt(pageId, moving, point);
    };
    const end = (): void => {
      this.#stop();
      [this.dragged, this.mark, this.at] = [undefined, undefined, undefined];
    };
    return {
      start: (point) => {
        moving = withDescendants(this.#source.pages(), pageId);
        this.dragged = pageId;
        show(point);
        this.#scroll(pageId, moving);
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
