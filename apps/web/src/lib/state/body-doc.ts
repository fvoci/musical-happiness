/**
 * A card's body opened in its room (`Backend.openDoc`): the shapes the
 * backends and the screens share. The document is Yjs's; only its type is
 * named here, so that nothing of Yjs comes with the backends.
 */
import type { Doc as YDoc } from 'yjs';

/**
 * How an opened body stands (`Backend.openDoc`):
 *
 * - `joining`: asking to be let into the card's room; no document yet.
 * - `joined`: in the room with the server's document (`mode` says whether
 *   the reader may write in it).
 * - `receiving`: a document is held and the server sends it again (let in
 *   again, or the document was made anew); nothing can be written meanwhile.
 * - `offline`: no connection to the server; it is joined again once there is.
 * - `out`: not in the room, for `reason` (a reason of the server's, or
 *   `not_loaded` when the code of the room could not be fetched). It is
 *   tried again by itself when `retrying`.
 */
export type BodyDocStatus = 'joining' | 'joined' | 'receiving' | 'offline' | 'out';

export interface BodyDocState {
  status: BodyDocStatus;
  /** The server's document, once one has arrived; another one after it was made anew. */
  doc: YDoc | undefined;
  /** `write` or `read`: as the server let the reader in. */
  mode: 'write' | 'read';
  /** Why it is `out`. */
  reason: string | undefined;
  /** The reader may change the document now. */
  writable: boolean;
  /** The reader changed the document and the server is not known to have it all yet. */
  pending: boolean;
  /** Being `out`, it is tried again by itself. */
  retrying: boolean;
}

/** What the opener of a body is told. */
export interface BodyDocHears {
  /** The state changed (`BodyDoc.state` is the new one). */
  state(state: BodyDocState): void;
  /**
   * What the reader wrote and the server never took was dropped with the
   * document it was in; `why` is the server's reason, or `document_reset`
   * when the server made the document anew.
   */
  discarded(why: string): void;
}

/** A card's body, open in its room. */
export interface BodyDoc {
  readonly state: BodyDocState;
  /** Asks to be let in again now, after being put out. */
  retry(): void;
  /** Sends what waits at once; settles when the server has it, or cannot have it now. */
  settle(): Promise<void>;
  /** Leaves the room; the document is let go. */
  close(): void;
}
