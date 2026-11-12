/**
 * Changes to the definitions of a board's card properties, sent through the
 * board store. What has to change is worked out in `property-change.ts`;
 * this file sends it: the board first, then the views and cards that go
 * with it, all under one undo group, so that one "undo" takes the whole
 * change back.
 *
 * There is no write that changes a board and many of its blocks at once
 * (the store has none, and the server design names none), so a change of
 * kind on a board with many values is many writes.
 */
import type { OptionCallbacks } from '$lib/components/properties/index.ts';
import type {
  Board,
  BoardView,
  PropertyOption,
  PropertyTemplate,
  PropertyType,
} from '$lib/logic/wire.ts';
import type { BoardStore } from '$lib/state/index.ts';

import {
  newOption,
  newProperty,
  viewWithoutProperty,
  type TypeChangePlan,
} from './property-change.ts';

const templateOf = (board: BoardStore, propertyId: string): PropertyTemplate | undefined =>
  board.board?.cardProperties.find((template) => template.id === propertyId);

/** The board with one of its property definitions replaced. */
const withTemplate =
  (propertyId: string, change: (template: PropertyTemplate) => PropertyTemplate) =>
  (board: Board): Board => ({
    ...board,
    cardProperties: board.cardProperties.map((template) =>
      template.id === propertyId ? change(template) : template,
    ),
  });

/**
 * Adds a property of a kind at the end of the list. A table that is being
 * looked at shows it at once, as a new column. Returns the identifier of the
 * property, or undefined when the board refused it.
 */
export async function addProperty(
  board: BoardStore,
  view: BoardView | undefined,
  type: PropertyType,
  typeName: string,
): Promise<string | undefined> {
  if (!board.board) return undefined;
  const template = newProperty(type, typeName, board.board.cardProperties);
  return board.inOneStep(async (groupId) => {
    await board.changeBoard(
      (current) => ({ ...current, cardProperties: [...current.cardProperties, template] }),
      'addProperty',
      groupId,
    );
    if (!templateOf(board, template.id)) return undefined;
    if (view?.fields.viewType === 'table') {
      await board.changeView(
        view.id,
        (current) => ({
          ...current,
          fields: {
            ...current.fields,
            visiblePropertyIds: [...current.fields.visiblePropertyIds, template.id],
          },
        }),
        'addProperty',
        groupId,
      );
    }
    return template.id;
  });
}

/** Gives a property another name. An empty name, or the one it has, changes nothing. */
export async function renameProperty(
  board: BoardStore,
  propertyId: string,
  name: string,
): Promise<void> {
  const trimmed = name.trim();
  if (trimmed === '' || templateOf(board, propertyId)?.name === trimmed) return;
  await board.changeBoard(
    withTemplate(propertyId, (template) => ({ ...template, name: trimmed })),
    'renameProperty',
  );
}

/**
 * Changes the kind of a property as planned: the definition, then the views
 * that leaned on the old kind and the values that have to be rewritten or
 * removed.
 */
export async function changePropertyType(board: BoardStore, plan: TypeChangePlan): Promise<void> {
  const { template, values } = plan;
  await board.inOneStep(async (groupId) => {
    await board.changeBoard(
      withTemplate(template.id, () => template),
      'changePropertyType',
      groupId,
    );
    // Refused: the views and the values stay as they are.
    if (templateOf(board, template.id)?.type !== template.type) return;
    await Promise.all([
      ...board.views.map((view) =>
        board.changeView(
          view.id,
          (current) => viewWithoutProperty(current, template.id, template.type),
          'changePropertyType',
          groupId,
        ),
      ),
      ...[...values].map(([cardId, value]) =>
        board.setCardProperty(cardId, template.id, value, groupId),
      ),
    ]);
  });
}

/**
 * Deletes a property: the definition, and every mention of it in the views.
 * The values stay on the cards, where nothing shows them any more: bringing
 * the property back (undo) brings them back with it, in one write.
 */
export async function deleteProperty(board: BoardStore, propertyId: string): Promise<void> {
  await board.inOneStep(async (groupId) => {
    await board.changeBoard(
      (current) => ({
        ...current,
        cardProperties: current.cardProperties.filter((template) => template.id !== propertyId),
      }),
      'deleteProperty',
      groupId,
    );
    if (templateOf(board, propertyId)) return;
    await Promise.all(
      board.views.map((view) =>
        board.changeView(
          view.id,
          (current) => viewWithoutProperty(current, propertyId),
          'deleteProperty',
          groupId,
        ),
      ),
    );
  });
}

/** What the editor of a select value reports about its options, applied to the board. */
export function optionCallbacks(board: BoardStore, propertyId: string): Required<OptionCallbacks> {
  const withOptions = (
    change: (options: PropertyOption[]) => PropertyOption[],
    description: string,
  ): Promise<void> =>
    board.changeBoard(
      withTemplate(propertyId, (template) => ({ ...template, options: change(template.options) })),
      description,
    );
  const replace =
    (option: PropertyOption, next: Partial<PropertyOption>) =>
    (options: PropertyOption[]): PropertyOption[] =>
      options.map((each) => (each.id === option.id ? { ...each, ...next } : each));
  return {
    onCreateOption: async (name) => {
      const option = newOption(name);
      await withOptions((options) => [...options, option], 'addOption');
      const made = templateOf(board, propertyId)?.options.some((each) => each.id === option.id);
      // The store has told the reader; the editor leaves the value alone.
      if (!made) throw new Error('the option was not made');
      return option;
    },
    onRenameOption: (option, name) => {
      void withOptions(replace(option, { value: name }), 'renameOption');
    },
    onRecolorOption: (option, color) => {
      void withOptions(replace(option, { color }), 'recolorOption');
    },
    onDeleteOption: (option) => {
      void withOptions(
        (options) => options.filter((each) => each.id !== option.id),
        'deleteOption',
      );
    },
  };
}
