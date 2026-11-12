/**
 * The app's addresses, as the tests write and read them: the one place that
 * knows their shape (`src/app/router.svelte.ts` is the app's).
 *
 * The world in memory has one workspace, made first by the seed, so its
 * identifier is the seed's first.
 */

/** The workspace of the world in memory. */
export const WORKSPACE = '01930000-0000-7000-8000-000000000001';

const ID = '[0-9a-f-]{36}';

/** The address of a board, a view of it, a card on that view (`/w/…/boards/…`). */
export function boardPath(boardId: string, viewId?: string, cardId?: string): string {
  const parts = [boardId, viewId, cardId].filter((part) => part !== undefined);
  return `/w/${WORKSPACE}/boards/${parts.map((part) => encodeURIComponent(part)).join('/')}`;
}

/** The address of a view of a board, and nothing after it. */
export function viewAddress(boardId: string, viewId: string): RegExp {
  return new RegExp(`${boardPath(boardId, viewId)}$`);
}

/** Any view of any board: what the address settles on when a board is shown. */
export const ON_A_VIEW = new RegExp(`/w/${ID}/boards/${ID}/${ID}$`);
/** Any card open over a view. */
export const ON_A_CARD = new RegExp(`/w/${ID}/boards/${ID}/${ID}/[^/]+$`);
/** The workspace of the world, a board and a view of it, and maybe a card: an address settled. */
export const SETTLED = new RegExp(`/w/${WORKSPACE}/boards/${ID}/${ID}(/[^/]+)?$`);
/** Any board, with or without a view. */
export const ON_A_BOARD = new RegExp(`/w/${ID}/boards/${ID}(/${ID})?$`);

/** The board and the view an address names; empty strings for what it does not. */
export function placeOf(address: string): { boardId: string; viewId: string } {
  const { pathname } = new URL(address, 'http://here');
  const [, boardId = '', viewId = ''] =
    /\/w\/[^/]+\/boards\/([^/]+)(?:\/([^/]+))?/.exec(pathname) ?? [];
  return { boardId, viewId };
}

/** The view an address names. */
export function viewIdOf(address: string): string {
  return placeOf(address).viewId;
}
