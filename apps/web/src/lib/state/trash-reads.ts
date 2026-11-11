/** Reading the trash, part of the data boundary (`backend.ts`). */

/** A card or a board in the trash: enough to decide whether to bring it back. */
export interface DeletedItem {
  id: string;
  /** Its title when it was deleted. */
  title: string;
  deletedAt: number;
  /** Who deleted it, who may no longer exist; empty if not known. */
  deletedBy: string;
}

/** A page of a list: `page` counts from 0, and a `perPage` of zero or less is all of it. */
export interface Paging {
  page: number;
  perPage: number;
}

/** What the backend lists of the trash, for whoever may bring it back. */
export interface TrashReads {
  /**
   * The team's boards in the trash that the reader was an admin of (who may
   * bring them back): a page, the last deleted first.
   */
  deletedBoards(teamId: string, paging?: Paging): Promise<DeletedItem[]>;
  /**
   * The board's cards in the trash, for its editors: a page, the last deleted
   * first. A card that went with the block it was on is not listed; it comes
   * back with that block.
   */
  deletedCards(boardId: string, paging?: Paging): Promise<DeletedItem[]>;
}
