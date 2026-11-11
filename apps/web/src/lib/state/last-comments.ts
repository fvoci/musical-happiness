/**
 * Keeps each card's `lastCommentAt` current between readings of the card
 * list, as the contract says a client does: a comment that comes is at
 * least as new as the card's last one, and after a comment is deleted the
 * card is read again. A change message of a card does not carry the time, so
 * a card that comes so keeps the one it had.
 *
 * A deletion message names the block only. The card of a comment is known
 * when the comment came while the board was open, or is on the open card;
 * a comment never seen is of no card this knows, and the card list is read
 * again for it.
 */
import type { CardSummary, Comment } from './backend.ts';

export class LastComments {
  /** The card of each comment heard of, by the comment. */
  readonly #cardOf = new Map<string, string>();
  /** Comments heard per card: a reading of the card begun before one came is not the newest. */
  readonly #heard = new Map<string, number>();

  /** A comment came (posted, changed or brought back): the card it is on, with the newer time. */
  commented(cards: readonly CardSummary[], comment: Comment): CardSummary[] {
    this.#cardOf.set(comment.id, comment.parentId);
    this.#heard.set(comment.parentId, (this.#heard.get(comment.parentId) ?? 0) + 1);
    return cards.map((card) =>
      card.id === comment.parentId && (card.lastCommentAt ?? 0) < comment.createAt
        ? { ...card, lastCommentAt: comment.createAt }
        : card,
    );
  }

  /** Learns the card of a comment the open card shows. */
  know(comment: Pick<Comment, 'id' | 'parentId'>): void {
    this.#cardOf.set(comment.id, comment.parentId);
  }

  /**
   * Blocks were deleted: the cards whose last comment may have gone, and
   * whether one is of no card known. A deletion that names a block `known`
   * (a card, a view, an attachment) took only that and what was under it.
   */
  deleted(
    blockIds: readonly string[],
    known: readonly string[],
  ): {
    cardIds: string[];
    unknown: boolean;
  } {
    const cardIds = new Set<string>();
    let unknown = false;
    // What went with a card went with it (the memory backend names them all).
    if (blockIds.some((id) => known.includes(id))) return { cardIds: [], unknown };
    for (const id of blockIds) {
      const cardId = this.#cardOf.get(id);
      if (cardId === undefined) unknown = true;
      else {
        cardIds.add(cardId);
        this.#cardOf.delete(id);
      }
    }
    return { cardIds: [...cardIds], unknown };
  }

  /** A mark to tell, when a reading of the card comes back, whether a comment came meanwhile. */
  mark(cardId: string): number {
    return this.#heard.get(cardId) ?? 0;
  }

  /** The time read of a card, unless a comment came after the reading began. */
  read(cards: readonly CardSummary[], cardId: string, at: number, mark: number): CardSummary[] {
    if (this.mark(cardId) !== mark) return [...cards];
    return cards.map((card) => (card.id === cardId ? { ...card, lastCommentAt: at } : card));
  }

  /** A card as a change message brings it: without the time, which it keeps. */
  static keep(incoming: CardSummary, before: CardSummary | undefined): CardSummary {
    if (incoming.lastCommentAt !== undefined || before?.lastCommentAt === undefined)
      return incoming;
    return { ...incoming, lastCommentAt: before.lastCommentAt };
  }
}
