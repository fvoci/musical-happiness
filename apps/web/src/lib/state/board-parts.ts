/**
 * What the board store is built on besides the backend: a board of the
 * workspace shown, a member's role, the flags of a role, the place of a drop.
 */
import type { Board, BoardMember, MemberRole } from '../logic/wire.ts';

import type { Backend, BoardRole } from './backend.ts';
import { BackendError } from './problems.ts';

/**
 * Reads a board of the workspace `workspaceId`, the one the address names.
 * A board of another workspace is not found there (404), as one that is not
 * there at all: the address does not say where else it is.
 */
export async function boardOfWorkspace(
  backend: Backend,
  boardId: string,
  workspaceId: string | undefined,
): Promise<Board> {
  const board = await backend.board(boardId);
  if (board.workspaceId !== workspaceId) {
    throw new BackendError(404, { detail: 'the board is of another workspace' });
  }
  return board;
}

export function roleOf(member: BoardMember | undefined): MemberRole {
  if (!member) return '';
  if (member.schemeAdmin) return 'admin';
  if (member.schemeEditor) return 'editor';
  if (member.schemeCommenter) return 'commenter';
  return member.schemeViewer ? 'viewer' : '';
}

/** The flags of a membership with exactly the one role. */
export function roleFlags(
  role: BoardRole,
): Pick<BoardMember, 'schemeAdmin' | 'schemeEditor' | 'schemeCommenter' | 'schemeViewer'> {
  return {
    schemeAdmin: role === 'admin',
    schemeEditor: role === 'editor',
    schemeCommenter: role === 'commenter',
    schemeViewer: role === 'viewer',
  };
}

/** Where dropped cards go: a column (by the value of the grouping property) and a place in it. */
export interface CardDrop {
  viewId: string;
  cardIds: readonly string[];
  /**
   * The option of the column they were dropped in; `''` is the column of
   * cards without a value. Undefined: the view is not grouped, or the column
   * is not to change.
   */
  optionId?: string;
  /**
   * The place among the cards of that column, counted without the dropped
   * ones. Undefined: the view is sorted and places itself.
   */
  index?: number;
  /**
   * The cards of that column the page shows, in shown order, when a filter,
   * a search or a folded group may hide some of them: `index` is then a
   * place among these, and the store finds the place among all of the
   * column (`placeAmongAll`). Left out: `index` counts every card.
   */
  shown?: readonly string[];
}
