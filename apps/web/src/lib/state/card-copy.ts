/**
 * A copy of a card of a board: title, icon, properties and body. Comments and
 * attachments are not copied. The source is read first: the copy is a step
 * from the moment it is asked for (an undo asked for meanwhile takes it
 * back, not the change before it), of the history it was asked in.
 */
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
      const detail = await board.workspace.backend.card(board.boardId, cardId);
      return await board.createCard(
        {
          title: detail.title,
          icon: detail.fields.icon,
          properties: detail.fields.properties,
          isTemplate: asTemplate ?? detail.fields.isTemplate,
          body: detail.body,
        },
        groupId,
      );
    } catch (error) {
      board.workspace.report('save', error);
      return undefined;
    }
  }, true);
}
