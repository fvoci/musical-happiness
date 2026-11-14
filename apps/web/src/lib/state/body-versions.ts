/**
 * The versions the server keeps of a card's body: when the last of those
 * who wrote it leave, at an interval while it is written on (ten minutes,
 * unless the server is set otherwise), and around a body written whole.
 * The newest are kept (fifty, unless the server is set otherwise): the
 * lowest numbers go as new versions come.
 */
import type { DocNode } from '../editor/schema.ts';

export interface BodyVersion {
  /** Its place among the card's, counted from 1. */
  version: number;
  /** When it was kept; of a body nobody is known to have written, when the card was last changed. */
  keptAt: number;
  /** Who wrote the body up to it since the version before; none for a body nobody is known to have written. */
  editors: string[];
  /**
   * Its body is the one the card has now. Of versions with the same body,
   * the newest; of none while what was typed is not written to the card yet.
   */
  isCurrent: boolean;
}

export interface BodyVersionContent extends BodyVersion {
  /** The body of the card then; `null` if it had none. */
  body: DocNode | null;
}

export interface BodyVersions {
  /** The versions of a card's body, the newest first, for the board's members. */
  list(boardId: string, cardId: string): Promise<BodyVersion[]>;
  /** One version with its body; 404 for one the card has no more. */
  read(boardId: string, cardId: string, version: number): Promise<BodyVersionContent>;
  /**
   * Writes a version's body to its card again, for who edits the board: the
   * card's room gets it as a new document, the body before stays a version,
   * and a card that has that body already is left as it is. 409 for a
   * version that cannot be stored any more.
   */
  restore(boardId: string, cardId: string, version: number): Promise<void>;
}

/** Where a backend keeps the versions of the bodies of its cards. */
export interface VersionsOfBodies {
  readonly bodyVersions: BodyVersions;
}
