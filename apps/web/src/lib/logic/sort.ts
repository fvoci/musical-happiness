/** The order of the cards of a view, and of the views of a board. */
import { diagnostics } from './diagnostics.ts';
import {
  cardProperty,
  parseDateObject,
  TITLE_COLUMN_ID,
  type BoardView,
  type Card,
  type PropertyTemplate,
  type SortOption,
} from './wire.ts';

/** By title; cards without a title go last, oldest first among themselves. */
export function titleOrCreatedOrder(first: Card, second: Card): number {
  if (first.title && second.title) return first.title.localeCompare(second.title);
  if (first.title && !second.title) return -1;
  if (second.title && !first.title) return 1;
  return first.createAt - second.createAt;
}

/**
 * By the position the cards were dragged to. Cards that were never placed
 * go after the placed ones, by title.
 *
 * QUIRK(Q1): the comparator is not symmetric. Comparing a card that has no
 * position with a placed one gives 1 ("after"), as intended, but the same
 * pair the other way round gives `index + 1`, also "after". The result then
 * depends on the order in which the sort happens to compare. Kept as in the
 * original.
 */
export function manualOrder(cardOrder: readonly string[], first: Card, second: Card): number {
  const indexFirst = cardOrder.indexOf(first.id);
  const indexSecond = cardOrder.indexOf(second.id);
  if (indexFirst < 0 && indexSecond < 0) return titleOrCreatedOrder(first, second);
  if (indexFirst < 0 && indexSecond >= 0) return 1;
  return indexFirst - indexSecond;
}

/** What sorting needs to know besides the cards. */
export interface SortContext {
  templates: readonly PropertyTemplate[];
  sortOptions: readonly SortOption[];
  /** The manual order, used when there are no sort options. */
  cardOrder: readonly string[];
  /** When the newest comment of a card was written, if it has comments. */
  lastCommentAt?: (cardId: string) => number | undefined;
  /** The username of a user, for sorting by person. */
  usernameOf?: (userId: string) => string | undefined;
  /** False while no user is known yet; person values then sort by identifier. */
  usersKnown?: boolean;
}

/**
 * The comparator for one sort option on a property.
 *
 * Cards without a value go last whichever way the sort runs: those
 * comparisons return before the reversal. Equal values are ordered by title.
 */
function byProperty(template: PropertyTemplate, reversed: boolean, context: SortContext) {
  const username = (userId: string): string => context.usernameOf?.(userId) ?? '';
  const finish = (result: number, first: Card, second: Card): number => {
    const decided = result === 0 ? titleOrCreatedOrder(first, second) : result;
    return reversed ? -decided : decided;
  };

  return (first: Card, second: Card): number => {
    let a: string | string[] = cardProperty(first, template.id) ?? '';
    let b: string | string[] = cardProperty(second, template.id) ?? '';

    switch (template.type) {
      case 'createdBy':
        a = username(first.createdBy);
        b = username(second.createdBy);
        break;
      case 'updatedBy':
        a = username(first.modifiedBy);
        b = username(second.modifiedBy);
        break;
      case 'date': {
        // The start of the value; a value that cannot be read sorts as empty.
        const start = (value: string | string[]): string => {
          const from = parseDateObject(typeof value === 'string' ? value : '').from;
          return from ? String(from) : '';
        };
        a = start(a);
        b = start(b);
        break;
      }
      case 'createdTime':
        return finish(first.createAt - second.createAt, first, second);
      case 'updatedTime': {
        const changed = (card: Card): number =>
          Math.max(card.updateAt, context.lastCommentAt?.(card.id) ?? 0);
        return finish(changed(first) - changed(second), first, second);
      }
      default:
    }

    if (a.length > 0 && b.length === 0) return -1;
    if (b.length > 0 && a.length === 0) return 1;
    if (a.length === 0 && b.length === 0) return titleOrCreatedOrder(first, second);

    if (template.type === 'number' || template.type === 'date') {
      return finish(Number(a) - Number(b), first, second);
    }
    if (template.type === 'select' || template.type === 'multiSelect') {
      const label = (value: string | string[]): string => {
        const id = Array.isArray(value) ? value[0] : value;
        return template.options.find((option) => option.id === id)?.value ?? '';
      };
      a = label(a);
      b = label(b);
    }
    if (template.type === 'multiPerson' && context.usersKnown !== false) {
      const names = (value: string | string[]): string =>
        Array.isArray(value) ? value.map(username).toString() : value;
      a = names(a);
      b = names(b);
    }
    return finish(String(a).localeCompare(String(b)), first, second);
  };
}

/**
 * The cards in the order the view asks for. The list passed in is not
 * changed.
 *
 * Without sort options the order is the manual one. Otherwise each option is
 * applied in turn to the whole list.
 *
 * QUIRK(Q6): because every option sorts the whole list again and breaks its
 * ties by title, only the last option has a visible effect; an earlier one
 * shows only between cards that are equal in the last option and in title.
 * Kept as in the original.
 */
export function sortCards(cards: readonly Card[], context: SortContext): Card[] {
  const sorted = [...cards];
  if (context.sortOptions.length < 1) {
    return sorted.sort((a, b) => manualOrder(context.cardOrder, a, b));
  }

  for (const option of context.sortOptions) {
    if (option.propertyId === TITLE_COLUMN_ID) {
      const direction = option.reversed ? -1 : 1;
      sorted.sort((a, b) => direction * titleOrCreatedOrder(a, b));
      continue;
    }
    const template = context.templates.find((candidate) => candidate.id === option.propertyId);
    if (!template) {
      diagnostics.report(`sortCards: no template for property ${option.propertyId}`);
      return sorted;
    }
    sorted.sort(byProperty(template, option.reversed, context));
  }
  return sorted;
}

/** Views by title, ignoring an emoji at the start of the title. */
export function sortBoardViewsAlphabetically(views: readonly BoardView[]): BoardView[] {
  return views
    .map((view) => ({ view, title: view.title.replace(/^\p{Emoji}*\s*/u, '') }))
    .sort((first, second) => first.title.localeCompare(second.title))
    .map(({ view }) => view);
}
