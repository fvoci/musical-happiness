/**
 * What the board store is built on besides the backend: a member's role,
 * the flags of a role, a change worked out when it is made, the place of a
 * drop.
 */
import { NO_CHANGE } from '../logic/undo.ts';
import type { BoardMember, MemberRole } from '../logic/wire.ts';

import type { BoardRole } from './backend.ts';

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

/**
 * The two halves of a change for `UndoManager.perform`, its patches worked
 * out by `work` when the change is made: now, or after an undo asked for
 * before it, from what that undo left. `work` answers nothing when there is
 * nothing left to change; the change is then no step. Redo and undo send the
 * patches worked out the first time.
 */
export function patchStep<P>(
  work: () => [P, P] | undefined,
  send: (patch: P, reverse: P) => Promise<void>,
): [redo: () => Promise<typeof NO_CHANGE | undefined>, undo: () => Promise<void>] {
  let patches: [P, P] | undefined;
  return [
    async () => {
      patches ??= work();
      if (!patches) return NO_CHANGE;
      await send(...patches);
      return undefined;
    },
    async () => {
      if (patches) await send(patches[1], patches[0]);
    },
  ];
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
