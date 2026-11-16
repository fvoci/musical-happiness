/**
 * The trash of the world in memory, as the server keeps its own: the cards
 * and boards deleted, with who deleted them and when, listed for whoever may
 * bring them back; and the places a deleted board had in its people's
 * sidebars, to file it there again when it comes back.
 */
import type { Container, ContainerMember } from '../../logic/wire.ts';
import type { Category, DeletedItem, Paging } from '../backend.ts';

import { fail } from './refuse.ts';
import type { World } from './world.ts';

/** Where a board was filed for one person: the category, the place in it, hidden or not. */
export interface ContainerPlace {
  userId: string;
  categoryId: string;
  index: number;
  hidden: boolean;
}

/** As the server has it when a request does not say. */
const DEFAULT_PAGING: Paging = { page: 0, perPage: 100 };

/** A page of a list, as the server cuts one: `perPage` zero or less is all of it. */
function pageOf<T>(items: T[], paging: Paging | undefined): T[] {
  const { page, perPage } = paging ?? DEFAULT_PAGING;
  if (perPage <= 0) return items;
  return items.slice(page * perPage, page * perPage + perPage);
}

const latestFirst = (first: DeletedItem, second: DeletedItem): number =>
  second.deletedAt - first.deletedAt;

/**
 * The cards of a board in the trash: those deleted by themselves. A card
 * that went with the block it was on comes back with that block, and is not
 * listed. The last deleted first.
 */
export function deletedCardsOf(
  world: World,
  boardId: string,
  paging: Paging | undefined,
): DeletedItem[] {
  const items: DeletedItem[] = [];
  for (const [id, deleted] of world.deletedBlocks) {
    const [first] = deleted.blocks;
    if (deleted.containerId !== boardId || first?.id !== id || first.type !== 'card') continue;
    items.push({
      id,
      title: first.title,
      deletedAt: deleted.deletedAt,
      deletedBy: deleted.deletedBy,
    });
  }
  return pageOf(items.sort(latestFirst), paging);
}

/**
 * The pages of a wiki in the trash at the top of what went with them, each
 * with how many pages below it went along. A page that went with the page it
 * was below comes back with that page, and is not listed. The last deleted
 * first, then by identifier, as the server lists them.
 */
export function deletedPagesOf(
  world: World,
  wikiId: string,
  paging: Paging | undefined,
): DeletedItem[] {
  const items: DeletedItem[] = [];
  for (const [id, deleted] of world.deletedBlocks) {
    const [first] = deleted.blocks;
    if (deleted.containerId !== wikiId || first?.id !== id || first.type !== 'page') continue;
    items.push({
      id,
      title: first.title,
      deletedAt: deleted.deletedAt,
      deletedBy: deleted.deletedBy,
      // The pages that went with it: its comments and theirs went too, and are no pages.
      below: deleted.blocks.filter((block) => block.type === 'page').length - 1,
    });
  }
  const order = (one: DeletedItem, other: DeletedItem): number =>
    latestFirst(one, other) || (one.id < other.id ? -1 : one.id > other.id ? 1 : 0);
  return pageOf(items.sort(order), paging);
}

/** Whether a member was an admin of the board, which bringing it back takes. */
export const wasAdmin = (members: readonly ContainerMember[], userId: string): boolean =>
  members.some((member) => member.userId === userId && member.schemeAdmin);

/** The boards and wikis of the workspace in the trash that the user was an admin of, the last deleted first. */
export function deletedContainersOf(
  world: World,
  userId: string,
  workspaceId: string,
  paging: Paging | undefined,
): DeletedItem[] {
  const items = [...world.deletedContainers.values()]
    .filter(
      ({ container, members }) =>
        container.workspaceId === workspaceId && wasAdmin(members, userId),
    )
    .map(({ container, deletedAt, deletedBy }) => ({
      id: container.id,
      title: container.title,
      deletedAt,
      deletedBy,
      kind: container.kind ?? 'board',
    }));
  return pageOf(items.sort(latestFirst), paging);
}

/** Where everybody had filed the board or wiki, read before it goes. */
export function placesOf(world: World, containerId: string): ContainerPlace[] {
  const places: ContainerPlace[] = [];
  for (const [userId, categories] of world.categories) {
    for (const category of categories) {
      const index = category.containerIds.indexOf(containerId);
      if (index < 0) continue;
      places.push({
        userId,
        categoryId: category.id,
        index,
        hidden: category.hiddenContainerIds.includes(containerId),
      });
    }
  }
  return places;
}

/**
 * Files a board that came back where each person had it: the same category
 * and place, hidden or not; their default category if that one went
 * meanwhile. A private board only for its members. Answers the people whose
 * sidebar changed.
 */
export function putBack(
  world: World,
  board: Container,
  places: readonly ContainerPlace[],
): string[] {
  const memberIds = (world.members.get(board.id) ?? []).map((member) => member.userId);
  const told: string[] = [];
  for (const place of places) {
    if (board.type !== 'O' && !memberIds.includes(place.userId)) continue;
    const theirs: Category[] = world.categories.get(place.userId) ?? [];
    const kept = theirs.find((category) => category.id === place.categoryId);
    const category = kept ?? theirs.find((each) => each.type === 'system');
    if (!category || category.containerIds.includes(board.id)) continue;
    if (kept)
      category.containerIds.splice(
        Math.min(place.index, category.containerIds.length),
        0,
        board.id,
      );
    else category.containerIds.push(board.id);
    if (place.hidden) category.hiddenContainerIds.push(board.id);
    told.push(place.userId);
  }
  return told;
}

/** Who deletes and when, as the trash keeps it with what was deleted. */
export function deletion(by: { world: World; userId: string }): {
  deletedAt: number;
  deletedBy: string;
} {
  return { deletedAt: by.world.now(), deletedBy: by.userId };
}

/**
 * As the server: the trash of a board's cards, and bringing one back, are
 * for its editors and admins; a private board the user is no member of is
 * not there for them.
 */
export function mayManageCards(
  world: World,
  userId: string,
  boardId: string,
  refusal: string,
): void {
  const board = world.containers.get(boardId);
  // A wiki has no cards: the server answers for one as for a board that is not there.
  if (!board || board.kind === 'wiki') fail(404, 'no such board');
  const member = world.members.get(boardId)?.find((each) => each.userId === userId);
  if (!member && board.type !== 'O') fail(404, 'no such board');
  if (!member?.schemeAdmin && !member?.schemeEditor) fail(403, refusal);
}

/**
 * Takes a board or a wiki, its blocks and its members out of the trash, as the server
 * does: for who was its admin (403 otherwise), and no such board for who
 * was no member (404). Each person who had filed it finds it where it was,
 * and is told. Answers the board.
 */
export function takeContainerBack(world: World, userId: string, containerId: string): Container {
  const deleted = world.deletedContainers.get(containerId);
  if (!deleted?.members.some((member) => member.userId === userId)) fail(404, 'no such board');
  if (!wasAdmin(deleted.members, userId)) fail(403, 'access denied to undelete board');
  world.deletedContainers.delete(containerId);
  world.containers.set(containerId, deleted.container);
  world.members.set(containerId, deleted.members);
  for (const block of deleted.blocks) world.blocks.set(block.id, block);
  for (const person of putBack(world, deleted.container, deleted.places)) {
    const categories = structuredClone(world.categories.get(person) ?? []);
    world.tellUsers([person], { kind: 'categories', categories });
  }
  return deleted.container;
}
