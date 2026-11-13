/** The open card's conflicts, edits and uploads. */
import type { DocNode } from '../editor/schema.ts';
import type { CardDetail } from './backend.ts';

export interface BodyConflict {
  /** What the reader wrote. */
  mine: DocNode | null;
  /** The card as it is stored now. */
  theirs: CardDetail;
}

/** How a change of a comment ended: stored, refused for a newer version, or failed (and told). */
export type CommentEdit = 'saved' | 'conflict' | 'failed';

export interface Upload {
  id: number;
  name: string;
  sentBytes: number;
  totalBytes: number;
  failed: boolean;
  /** Gives the upload up. */
  cancel: () => void;
}
