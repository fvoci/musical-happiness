/**
 * The order things are shown in.
 *
 * No answer of the backend comes in an order to rely on, so every list a
 * screen shows is sorted here, and every comparison goes on until it has
 * told two different things apart (the identifier settles what is left).
 * The order of the cards of a view is the business of `logic/sort.ts`.
 */
import type { Block, Container, ContainerMember, BoardView } from '../logic/wire.ts';

import type { Category } from './backend.ts';

const byId = (first: { id: string }, second: { id: string }): number =>
  first.id < second.id ? -1 : first.id > second.id ? 1 : 0;

/** Oldest first; the identifier settles a tie. */
export function byCreation<T extends Pick<Block, 'id' | 'createAt'>>(items: readonly T[]): T[] {
  return [...items].sort(
    (first, second) => first.createAt - second.createAt || byId(first, second),
  );
}

function textOrder(locale: string): (first: string, second: string) => number {
  const collator = new Intl.Collator(locale, { numeric: true, sensitivity: 'base' });
  return (first, second) => collator.compare(first, second);
}

/** By title in the reader's language, untitled ones last, then oldest first. */
export function byTitle<T extends Pick<Block, 'id' | 'createAt' | 'title'>>(
  items: readonly T[],
  locale: string,
): T[] {
  const compare = textOrder(locale);
  return [...items].sort((first, second) => {
    if ((first.title === '') !== (second.title === '')) return first.title === '' ? 1 : -1;
    return (
      compare(first.title, second.title) || first.createAt - second.createAt || byId(first, second)
    );
  });
}

/** The views of a board in the order of their tabs. */
export function orderViews(views: readonly BoardView[], locale: string): BoardView[] {
  return byTitle(views, locale);
}

/** The members of a board by the name they are shown under. */
export function orderMembers(
  members: readonly ContainerMember[],
  nameOf: (userId: string) => string,
  locale: string,
): ContainerMember[] {
  const compare = textOrder(locale);
  return [...members].sort(
    (first, second) =>
      compare(nameOf(first.userId), nameOf(second.userId)) ||
      (first.userId < second.userId ? -1 : first.userId > second.userId ? 1 : 0),
  );
}

export interface SidebarCategory {
  category: Category;
  containers: Container[];
}

/**
 * The sidebar: the categories in the reader's order, each with its boards in
 * the reader's order. A board that no category names (it was just made, or
 * shared with the reader) goes to the end of the default category, by title.
 * A category naming a board the reader cannot see simply does not show it.
 */
export function sidebarOf(
  categories: readonly Category[],
  containers: readonly Container[],
  locale: string,
): SidebarCategory[] {
  const compare = textOrder(locale);
  const ordered = [...categories].sort(
    (first, second) =>
      first.sortOrder - second.sortOrder || compare(first.name, second.name) || byId(first, second),
  );
  const known = new Map(containers.map((board) => [board.id, board]));
  const filed = new Set<string>();
  const result = ordered.map((category) => {
    const own: Container[] = [];
    for (const boardId of category.containerIds) {
      const board = known.get(boardId);
      if (!board || filed.has(boardId) || category.hiddenContainerIds.includes(boardId)) continue;
      filed.add(boardId);
      own.push(board);
    }
    return { category, containers: own };
  });
  const hidden = new Set(categories.flatMap((category) => category.hiddenContainerIds));
  const loose = byTitle(
    containers.filter((board) => !filed.has(board.id) && !hidden.has(board.id)),
    locale,
  );
  const fallback = result.find((entry) => entry.category.type === 'system') ?? result.at(-1);
  fallback?.containers.push(...loose);
  return result;
}

/** The boards a category hides that the sidebar does not show elsewhere, by title. */
export function hiddenContainersOf(
  categories: readonly Category[],
  containers: readonly Container[],
  sidebar: readonly SidebarCategory[],
  locale: string,
): Container[] {
  const hidden = new Set(categories.flatMap((category) => category.hiddenContainerIds));
  const shown = new Set(sidebar.flatMap((entry) => entry.containers.map((board) => board.id)));
  return byTitle(
    containers.filter((board) => hidden.has(board.id) && !shown.has(board.id)),
    locale,
  );
}

/** The categories without a board that is gone: it is filed and hidden nowhere. */
export function withoutContainer(categories: readonly Category[], containerId: string): Category[] {
  return categories.map((category) => ({
    ...category,
    containerIds: category.containerIds.filter((id) => id !== containerId),
    hiddenContainerIds: category.hiddenContainerIds.filter((id) => id !== containerId),
  }));
}
