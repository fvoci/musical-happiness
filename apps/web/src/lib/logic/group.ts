/** The columns of a board view: cards grouped by the value of one property. */
import { cardProperty, type Card, type PropertyOption, type PropertyTemplate } from './wire.ts';

export interface BoardGroup {
  option: PropertyOption;
  cards: Card[];
}

export interface BoardGroups {
  visible: BoardGroup[];
  hidden: BoardGroup[];
}

/** The label of the group of cards that have no value: "No Status". */
export type NoValueLabel = (propertyName: string) => string;

function groupCardsByOptions(
  cards: readonly Card[],
  optionIds: readonly string[],
  groupBy: PropertyTemplate | undefined,
  noValueLabel: NoValueLabel,
): BoardGroup[] {
  const groups: BoardGroup[] = [];
  for (const optionId of optionIds) {
    if (optionId) {
      // An identifier without an option is a deleted option and gets no column.
      const option = groupBy?.options.find((candidate) => candidate.id === optionId);
      if (option && groupBy) {
        groups.push({
          option,
          cards: cards.filter((card) => cardProperty(card, groupBy.id) === optionId),
        });
      }
      continue;
    }
    // The empty identifier stands for the cards without a value, or with one that no longer exists.
    groups.push({
      option: { id: '', value: noValueLabel(groupBy?.name ?? ''), color: '' },
      cards: cards.filter((card) => {
        const value = cardProperty(card, groupBy?.id ?? '');
        return !value || !groupBy?.options.some((option) => option.id === value);
      }),
    });
  }
  return groups;
}

function getOptionGroups(
  cards: readonly Card[],
  visibleOptionIds: readonly string[],
  hiddenOptionIds: readonly string[],
  groupBy: PropertyTemplate | undefined,
  noValueLabel: NoValueLabel,
): BoardGroups {
  // An option the view does not mention yet is shown, after the mentioned ones.
  const unassigned = (groupBy?.options ?? [])
    .map((option) => option.id)
    .filter((id) => !visibleOptionIds.includes(id) && !hiddenOptionIds.includes(id));
  const visible = [...visibleOptionIds, ...unassigned];
  // Unless the view placed it, the column of cards without a value comes first.
  if (!visible.includes('') && !hiddenOptionIds.includes('')) visible.unshift('');

  return {
    visible: groupCardsByOptions(cards, visible, groupBy, noValueLabel),
    hidden: groupCardsByOptions(cards, hiddenOptionIds, groupBy, noValueLabel),
  };
}

/**
 * One group per person that occurs, in the order of first occurrence.
 *
 * QUIRK(Q4): a card without a person lands in a group whose identifier and
 * label are the text "undefined". Kept as in the original.
 */
function getPersonGroups(
  cards: readonly Card[],
  groupBy: PropertyTemplate,
  hiddenOptionIds: readonly string[],
): BoardGroups {
  const byPerson = new Map<string, Card[]>();
  for (const card of cards) {
    let key = String(cardProperty(card, groupBy.id));
    if (groupBy.type === 'createdBy') key = card.createdBy;
    else if (groupBy.type === 'updatedBy') key = card.modifiedBy;
    const group = byPerson.get(key);
    if (group) group.push(card);
    else byPerson.set(key, [card]);
  }

  const groups: BoardGroups = { visible: [], hidden: [] };
  for (const [key, groupCards] of byPerson) {
    const group = { option: { id: key, value: key, color: '' }, cards: groupCards };
    // A group with an empty key (no creator recorded) is never hidden, as in the original.
    if (key !== '' && hiddenOptionIds.includes(key)) groups.hidden.push(group);
    else groups.visible.push(group);
  }
  return groups;
}

/** The visible and the hidden columns of a view, each with its cards in the order given. */
export function getVisibleAndHiddenGroups(
  cards: readonly Card[],
  visibleOptionIds: readonly string[],
  hiddenOptionIds: readonly string[],
  groupBy: PropertyTemplate | undefined,
  noValueLabel: NoValueLabel,
): BoardGroups {
  if (
    groupBy &&
    (groupBy.type === 'createdBy' || groupBy.type === 'updatedBy' || groupBy.type === 'person')
  ) {
    return getPersonGroups(cards, groupBy, hiddenOptionIds);
  }
  return getOptionGroups(cards, visibleOptionIds, hiddenOptionIds, groupBy, noValueLabel);
}
