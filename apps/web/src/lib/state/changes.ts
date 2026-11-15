/** The changes the backend tells as they happen, part of the data boundary (`backend.ts`). */
import type { Board, BoardMember, BoardView } from '../logic/wire.ts';

import type { AccountSettings, Attachment, CardSummary, Category, Comment } from './backend.ts';
import type { WikiPage } from './wiki-types.ts';

/**
 * A change that happened on the server, whoever made it: the reader in this
 * tab, the reader in another one, or somebody else. Stores apply these the
 * same way they apply the answer to their own write, so applying one twice
 * does nothing.
 */
export type Change =
  | { kind: 'board'; board: Board }
  | { kind: 'boardDeleted'; boardId: string }
  /** Blocks that were added or changed. A card comes as its summary. */
  | { kind: 'blocks'; boardId: string; blocks: (BoardView | CardSummary | Comment | Attachment)[] }
  /**
   * Blocks deleted: only the one the deletion was asked for, not what went
   * with it (the cards of a view do not go; the pages below a page do).
   */
  | { kind: 'blocksDeleted'; boardId: string; blockIds: string[] }
  /** Pages of a wiki (`boardId`) that were made or changed, without their bodies. */
  | { kind: 'pages'; boardId: string; pages: WikiPage[] }
  | { kind: 'members'; boardId: string; members: BoardMember[] }
  | { kind: 'categories'; categories: Category[] }
  | { kind: 'settings'; settings: AccountSettings }
  /**
   * Changes may have been missed (the connection to the server dropped and
   * is back): read again what is shown. The memory backend never sends it.
   */
  | { kind: 'resync' };

export type ChangeListener = (change: Change) => void;

type ChangedBlock = Extract<Change, { kind: 'blocks' }>['blocks'][number];

/** Whether a changed block is a card. The types of the blocks overlap, so `type` has to be asked. */
export function isCardBlock(block: ChangedBlock): block is CardSummary {
  return block.type === 'card';
}

export function isViewBlock(block: ChangedBlock): block is BoardView {
  return block.type === 'view';
}

/** Whether a changed block is a comment, of a card or of a page. */
export function isCommentBlock(block: ChangedBlock): block is Comment {
  return block.type === 'comment';
}
