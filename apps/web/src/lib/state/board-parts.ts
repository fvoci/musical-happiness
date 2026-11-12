/**
 * What the board store is built on besides the backend: a member's role,
 * the flags of a role, the name of an undo group, the place of a drop, the
 * card a copy is made as.
 */
import { valuesOfCopy } from '../logic/dependencies.ts';
import type { Board, BoardMember, Card, MemberRole } from '../logic/wire.ts';

import type { BoardRole, CardDetail, CardDraft } from './backend.ts';

/**
 * The new card a copy of `source` on its board is made as, a card template
 * if `asTemplate` says so (as the source is, if it says nothing). Its
 * relations name the cards of `cards` its source named ([valuesOfCopy]).
 */
export function draftOfCopy(
  source: CardDetail,
  board: Board | undefined,
  cards: readonly Card[],
  asTemplate: boolean | undefined,
): CardDraft {
  return {
    title: source.title,
    icon: source.fields.icon,
    properties: valuesOfCopy(
      source,
      board?.cardProperties ?? [],
      new Set(cards.map(({ id }) => id)),
    ),
    isTemplate: asTemplate ?? source.fields.isTemplate,
    body: source.body,
  };
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

let groupCounter = 0;
/** A name for the changes of one gesture, so that undo takes them back together. */
export function newUndoGroup(): string {
  groupCounter += 1;
  return `group-${groupCounter}`;
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
