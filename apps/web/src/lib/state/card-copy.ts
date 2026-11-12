/**
 * A copy of a card of a board: title, icon, properties and body
 * (`draftOfCopy`). Comments and attachments are not copied. The source is
 * read first: the copy is a step from the moment it is asked for (an undo
 * asked for meanwhile takes it back, not the change before it), of the
 * history it was asked in.
 */
import { draftOfCopy } from './board-parts.ts';
import type { BoardStore } from './board.svelte.ts';
import type { CardSummary } from './backend.ts';

export function copyOfCard(
  board: BoardStore,
  cardId: string,
  asTemplate?: boolean,
): Promise<CardSummary | undefined> {
  if (!board.card(cardId)) return Promise.resolve(undefined);
  return board.inOneStep(async (groupId) => {
    try {
      return await board.createCard(
        await draftOfCopy(board.workspace.backend, board, cardId, asTemplate),
        groupId,
      );
    } catch (error) {
      board.workspace.report('save', error);
      return undefined;
    }
  }, true);
}
