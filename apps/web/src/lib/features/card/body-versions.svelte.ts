/**
 * The versions of an open card's body, as their dialog reads them: the
 * list, read when this is made (the dialog makes one each time it opens),
 * the version chosen with its body, and going back to it.
 *
 * Reading the list and restoring send what the reader typed to the card's
 * room first: the list then says of no version that it is the card's body
 * while the reader's own typing is not in it, and the server keeps what was
 * typed with the body before as a version. What could not be sent in time
 * (no connection) is dropped with the document the restore replaces, and
 * said lost as for any body written whole.
 */
import {
  isBackendError,
  type BoardStore,
  type BodyVersion,
  type BodyVersionContent,
  type CardStore,
} from '$lib/state/index.ts';

/**
 * Why a version was not restored: the card has it no more (`gone`), it
 * cannot be stored any more or shows an image whose file is gone
 * (`unrestorable`), or the write did not go through.
 */
export type RestoreRefusal = 'gone' | 'unrestorable' | 'failed';

export class VersionsReading {
  status = $state<'loading' | 'ready' | 'failed'>('loading');
  /** The newest first. */
  versions = $state.raw<BodyVersion[]>([]);
  /** The version being read, as the list has it. */
  chosen = $state.raw<BodyVersion>();
  /** Its body, once it is here. */
  content = $state.raw<BodyVersionContent>();
  /** `gone`: the card has the version no more (the newest are kept). */
  contentStatus = $state<'loading' | 'ready' | 'failed' | 'gone'>('loading');
  restoring = $state(false);
  refusal = $state<RestoreRefusal>();

  readonly #board: BoardStore;
  readonly #card: CardStore;

  constructor(board: BoardStore, card: CardStore) {
    this.#board = board;
    this.#card = card;
    void this.read();
  }

  get #versions(): BoardStore['workspace']['backend']['bodyVersions'] {
    return this.#board.workspace.backend.bodyVersions;
  }

  /** Reads the list (again). */
  async read(): Promise<void> {
    this.status = 'loading';
    try {
      await this.#card.settleBody();
      const list = await this.#versions.list(this.#board.boardId, this.#card.cardId);
      await this.#board.workspace.learnUsers(list.flatMap((one) => one.editors));
      this.versions = list;
      this.status = 'ready';
    } catch {
      this.status = 'failed';
    }
  }

  /**
   * Reads a version of the list; one chosen meanwhile takes its place. Not
   * while a version is being restored: its answer is about the one chosen.
   */
  async choose(version: BodyVersion): Promise<void> {
    if (this.restoring) return;
    this.chosen = version;
    this.content = undefined;
    this.contentStatus = 'loading';
    this.refusal = undefined;
    try {
      const content = await this.#versions.read(
        this.#board.boardId,
        this.#card.cardId,
        version.version,
      );
      if (this.chosen !== version) return;
      this.content = content;
      this.contentStatus = 'ready';
    } catch (error) {
      if (this.chosen !== version) return;
      this.contentStatus = statusOf(error) === 404 ? 'gone' : 'failed';
    }
  }

  /** Back to the list. */
  leave(): void {
    if (this.restoring) return;
    this.chosen = undefined;
    this.content = undefined;
    this.refusal = undefined;
  }

  /** Writes the chosen version to the card again; whether the server took it. */
  async restore(): Promise<boolean> {
    const version = this.chosen;
    if (!version || this.restoring) return false;
    this.restoring = true;
    this.refusal = undefined;
    try {
      await this.#card.settleBody();
      await this.#versions.restore(this.#board.boardId, this.#card.cardId, version.version);
      return true;
    } catch (error) {
      const status = statusOf(error);
      this.refusal = status === 404 ? 'gone' : status === 409 ? 'unrestorable' : 'failed';
      return false;
    } finally {
      this.restoring = false;
    }
  }
}

function statusOf(error: unknown): number {
  return isBackendError(error) ? error.status : 0;
}
