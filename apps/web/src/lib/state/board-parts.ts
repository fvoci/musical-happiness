/**
 * What the board store is built on besides the backend: a board of the
 * workspace shown, a member's role, the flags of a role, adding a member,
 * the place of a drop, the card a copy is made as.
 */
import { valuesOfCopy } from '../logic/dependencies.ts';
import {
  cardProperty,
  parseRelationValue,
  type Board,
  type BoardMember,
  type Card,
  type MemberRole,
} from '../logic/wire.ts';

import type { Backend, BoardRole, CardDraft, Paging } from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
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

/** Every card in the trash, in one page: a page size of zero is all of them. */
const ALL: Paging = { page: 0, perPage: 0 };

/** What a copy of a card is made from: the board it is on, as its store has it. */
interface CopiedFrom {
  boardId: string;
  board: Board | undefined;
  cards: readonly Card[];
}

/**
 * The new card a copy of the card `cardId` of `from` is made as, read whole
 * from the backend: a card template if `asTemplate` says so (as the source
 * is, if it says nothing). Its relations name the cards its source named
 * that are on the board: those of the store, and those in the board's trash,
 * as the server's copy of a card keeps them ([valuesOfCopy]). The trash is
 * read only when the source names a card the store does not have.
 *
 * It keeps less than the server's copy does: the trash lists the cards
 * deleted by themselves, not those that went with the block they were on,
 * and a trash that cannot be read names none.
 */
export async function draftOfCopy(
  backend: Pick<Backend, 'card' | 'deletedCards'>,
  from: CopiedFrom,
  cardId: string,
  asTemplate: boolean | undefined,
): Promise<CardDraft> {
  const source = await backend.card(from.boardId, cardId);
  const templates = from.board?.cardProperties ?? [];
  const onBoard = new Set(from.cards.map(({ id }) => id));
  const named = templates
    .filter((template) => template.type === 'relation')
    .flatMap((template) => parseRelationValue(cardProperty(source, template.id)));
  if (named.some((id) => !onBoard.has(id))) {
    // Unread, the trash names nothing: the copy then leaves those cards out.
    const trash = await backend.deletedCards(from.boardId, ALL).catch(() => []);
    for (const item of trash) onBoard.add(item.id);
  }
  return {
    title: source.title,
    icon: source.fields.icon,
    properties: valuesOfCopy(source, templates, onBoard),
    isTemplate: asTemplate ?? source.fields.isTemplate,
    body: source.body,
  };
}

export function roleOf(member: BoardMember | undefined): MemberRole {
  if (!member) return '';
  if (member.schemeAdmin) return 'admin';
  if (member.schemeEditor || member.inheritedRole === 'editor') return 'editor';
  if (member.schemeCommenter || member.inheritedRole === 'commenter') return 'commenter';
  return member.schemeViewer || member.inheritedRole === 'viewer' ? 'viewer' : '';
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

/** Makes a person of the workspace a member of `board`; one who is a member already keeps their role. */
export async function addMemberTo(
  board: BoardStore,
  userId: string,
  role: BoardRole,
): Promise<void> {
  try {
    const member = await board.workspace.backend.addMember(board.boardId, userId, role);
    board.members = [...board.members.filter((one) => one.userId !== userId), member];
    void board.workspace.learnUsers([userId], { members: true });
  } catch (error) {
    board.workspace.report('save', error);
  }
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
