/**
 * Cards dropped on a view, as one step of undo: into a column (their
 * grouping property takes the column's value) and, in a view without a
 * sort, at a place (the view's manual order changes).
 */
import { manualComparator } from '../logic/sort.ts';
import type { Card } from '../logic/wire.ts';

import type { CardDrop } from './board-parts.ts';
import type { BoardStore } from './board.svelte.ts';
import { droppedCardOrder, placeAmongAll } from './card-order.ts';

export async function dropCardsOn(board: BoardStore, drop: CardDrop): Promise<void> {
  const view = board.view(drop.viewId);
  if (!view) return;
  await board.inOneStep(async (groupId) => {
    const groupBy = board.groupingOf(view)?.id;
    const optionId = drop.optionId;
    const valueOf = (card: Card): string => {
      const value = groupBy ? card.fields.properties[groupBy] : undefined;
      return typeof value === 'string' ? value : '';
    };

    if (drop.index !== undefined) {
      // The cards as they stand in the manual order, the dropped ones taken out.
      const shown = [...board.realCards].sort(manualComparator(view.fields.cardOrder));
      const moved = drop.cardIds;
      const column = shown
        .filter((card) => !moved.includes(card.id))
        .filter((card) => optionId === undefined || valueOf(card) === optionId)
        .map((card) => card.id);
      const cardOrder = droppedCardOrder(
        shown.map((card) => card.id),
        column,
        // The dropped cards keep the order they had.
        shown.filter((card) => moved.includes(card.id)).map((card) => card.id),
        drop.shown
          ? placeAmongAll(
              column,
              drop.shown.filter((id) => !moved.includes(id)),
              drop.index,
            )
          : drop.index,
      );
      await board.changeView(
        view.id,
        (current) => ({ ...current, fields: { ...current.fields, cardOrder } }),
        'moveCard',
        groupId,
      );
    }

    if (groupBy && optionId !== undefined) {
      for (const cardId of drop.cardIds) {
        const card = board.card(cardId);
        if (!card || valueOf(card) === optionId) continue;
        await board.setCardProperty(
          cardId,
          groupBy,
          optionId === '' ? undefined : optionId,
          groupId,
        );
      }
    }
  });
}
