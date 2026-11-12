/** A copy of a card of a board: title, icon, properties and body. Comments and attachments are not copied. */
import type { BoardStore } from './board.svelte.ts';
import type { CardSummary } from './backend.ts';

export async function copyOfCard(
  board: BoardStore,
  cardId: string,
  asTemplate?: boolean,
): Promise<CardSummary | undefined> {
  if (!board.card(cardId)) return undefined;
  try {
    const detail = await board.workspace.backend.card(board.boardId, cardId);
    return await board.createCard({
      title: detail.title,
      icon: detail.fields.icon,
      properties: detail.fields.properties,
      isTemplate: asTemplate ?? detail.fields.isTemplate,
      body: detail.body,
    });
  } catch (error) {
    board.workspace.report('save', error);
    return undefined;
  }
}
