/**
 * One open card: its body, comments and attachments.
 *
 * The title, icon and properties of the card are not here: they are in the
 * card list of the board store (`summary`), so that the card and the board
 * behind it never disagree.
 *
 * The body is one document and is written whole, with the version it was
 * read at. When the card changed in between, the write is refused (409):
 *
 * - if the body itself is still what this card store last knew, only
 *   something else about the card changed, and the write is sent again on
 *   the new version without bothering the reader;
 * - otherwise somebody else wrote the body. Nothing is overwritten: the
 *   store holds both (`conflict`) and the screen asks the reader which to
 *   keep.
 */
import type { DocNode } from '../editor/schema.ts';

import {
  isBackendError,
  isConflict,
  type Attachment,
  type CardDetail,
  type CardSummary,
  type Comment,
} from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import { byCreation } from './order.ts';
import type { LoadStatus } from './workspace.svelte.ts';
import { reread, take } from './write.ts';

export interface BodyConflict {
  /** What the reader wrote. */
  mine: DocNode | null;
  /** The card as it is stored now. */
  theirs: CardDetail;
}

export interface Upload {
  id: number;
  name: string;
  sentBytes: number;
  totalBytes: number;
  failed: boolean;
  /** Gives the upload up. */
  cancel: () => void;
}

const sameDoc = (first: DocNode | null, second: DocNode | null): boolean =>
  JSON.stringify(first) === JSON.stringify(second);

export class CardStore {
  readonly board: BoardStore;
  readonly cardId: string;

  status = $state<LoadStatus>('idle');
  /** Readings again (`refresh`) begun: the latest is the one that counts. */
  #refreshing = 0;
  /** The card is gone (deleted here or by somebody else), or never was. */
  missing = $state(false);
  /** The body as it is stored, as far as this store knows. */
  body = $state.raw<DocNode | null>(null);
  /**
   * Goes up whenever `body` was replaced by something the reader did not
   * type: the first load, somebody else's change, the other side of a
   * conflict. An editor takes the body again when this changes.
   */
  bodyRevision = $state(0);
  /** A body is being written, or waits to be. */
  saving = $state(false);
  /** The reader wrote something that is not stored yet. */
  dirty = $state(false);
  conflict = $state.raw<BodyConflict | undefined>(undefined);
  uploads = $state.raw<Upload[]>([]);

  #comments = $state.raw<Comment[]>([]);
  #attachments = $state.raw<Attachment[]>([]);
  /** Oldest first. */
  readonly comments = $derived(byCreation(this.#comments));
  readonly attachments = $derived(byCreation(this.#attachments));

  /** Title, icon and properties, from the board's list. */
  readonly summary = $derived.by<CardSummary | undefined>(() => this.board.card(this.cardId));

  /** The version of the card the stored body was read at. */
  #readAt = 0;
  /** What the reader wrote and has not been stored yet. */
  #unsaved: { body: DocNode | null } | undefined;
  #writing: Promise<void> | undefined;
  #uploadCount = 0;
  #closed = false;

  constructor(board: BoardStore, cardId: string) {
    this.board = board;
    this.cardId = cardId;
  }

  get #backend(): BoardStore['workspace']['backend'] {
    return this.board.workspace.backend;
  }

  get #boardId(): string {
    return this.board.boardId;
  }

  async load(): Promise<void> {
    this.status = 'loading';
    try {
      const [detail, comments, attachments] = await Promise.all([
        this.#backend.card(this.#boardId, this.cardId),
        this.#backend.comments(this.#boardId, this.cardId),
        this.#backend.attachments(this.#boardId, this.cardId),
      ]);
      this.#takeDetail(detail);
      this.#comments = comments;
      this.#attachments = attachments;
      this.status = 'ready';
    } catch (error) {
      this.status = 'failed';
      if (isBackendError(error) && error.status === 404) this.missing = true;
      else this.board.workspace.report('load', error);
    }
  }

  close(): void {
    this.#closed = true;
    for (const upload of this.uploads) upload.cancel();
  }

  #takeDetail(detail: CardDetail): void {
    const { body, ...summary } = detail;
    this.#readAt = detail.updateAt;
    this.missing = false;
    this.board.receiveCard(summary);
    if (!sameDoc(body, this.body) || this.bodyRevision === 0) {
      this.body = body;
      this.bodyRevision += 1;
    }
  }

  // --------------------------------------------------------------- body

  /**
   * The reader changed the body. Nothing is sent yet: the screen calls this
   * on every change and `saveBody` when it is time to store (a pause in the
   * typing, leaving the field, closing the card). From this moment the body
   * counts as unsaved, and what arrives from other people does not replace
   * it on screen.
   */
  editBody(body: DocNode | null): void {
    this.#unsaved = { body };
    this.dirty = true;
  }

  /**
   * Stores the body: the one given, or the last one `editBody` was told.
   * Writes do not overlap: while one is under way the newest body waits and
   * goes next. Returns when everything given so far is stored, refused, or
   * waiting for the reader in `conflict`.
   */
  async saveBody(body?: DocNode | null): Promise<void> {
    if (body !== undefined) this.editBody(body);
    if (!this.#unsaved) {
      // Nothing new to send, but what was given before may still be on its way.
      await this.#writing;
      return;
    }
    this.saving = true;
    this.#writing ??= this.#drain().finally(() => {
      this.#writing = undefined;
      this.saving = false;
    });
    await this.#writing;
  }

  async #drain(): Promise<void> {
    while (this.#unsaved && !this.conflict && !this.#closed) {
      const { body } = this.#unsaved;
      this.#unsaved = undefined;
      try {
        await this.#write(body, false);
      } catch (error) {
        // Not stored: it stays unsaved, and the next `saveBody` sends it again.
        this.#unsaved ??= { body };
        this.board.workspace.report('save', error);
        return;
      }
    }
    this.dirty = this.#unsaved !== undefined || this.conflict !== undefined;
  }

  async #write(body: DocNode | null, retried: boolean): Promise<void> {
    // Always the version the body was read at, never a newer one the card
    // list has heard of: that one may carry somebody else's body. When the
    // newer version is only the reader's own change of a property, the write
    // is refused once and sent again below.
    const readAt = this.#readAt;
    try {
      const stored = await this.#backend.writeCardBody(this.#boardId, this.cardId, {
        body,
        readAt,
      });
      const { body: storedBody, ...summary } = stored;
      this.#readAt = stored.updateAt;
      this.body = storedBody;
      this.board.receiveCard(summary);
    } catch (error) {
      if (!isConflict(error)) throw error;
      const theirs = await this.#backend.card(this.#boardId, this.cardId);
      if (!retried && sameDoc(theirs.body, this.body)) {
        // Only something else about the card changed. Same body, new version: send again.
        const { body: _body, ...summary } = theirs;
        this.#readAt = theirs.updateAt;
        this.board.receiveCard(summary);
        await this.#write(body, true);
        return;
      }
      this.conflict = { mine: body, theirs };
      this.board.workspace.report('conflict', error);
    }
  }

  /**
   * Settles a conflict. `mine` stores what the reader wrote over what is
   * there now; `theirs` drops what the reader wrote and shows what is stored.
   */
  async resolveConflict(keep: 'mine' | 'theirs'): Promise<void> {
    const conflict = this.conflict;
    if (!conflict) return;
    this.conflict = undefined;
    const { body: theirBody, ...summary } = conflict.theirs;
    this.#readAt = conflict.theirs.updateAt;
    this.board.receiveCard(summary);
    this.body = theirBody;
    if (keep === 'theirs') {
      this.#unsaved = undefined;
      this.dirty = false;
      this.bodyRevision += 1;
      return;
    }
    await this.saveBody(conflict.mine);
  }

  /**
   * The card changed in the board's list. If that is news, and the reader
   * has nothing unsaved, the body is read again so that somebody else's
   * writing shows up while the card is open.
   */
  cardChanged(card: CardSummary): void {
    // A card that was deleted and is back (restored) is read again whatever its time.
    const back = this.missing;
    if (card.id !== this.cardId || (!back && card.updateAt <= this.#readAt)) return;
    if (this.dirty || this.saving || this.#closed) return;
    void this.#backend
      .card(this.#boardId, this.cardId)
      .then((detail) => {
        if (this.dirty || this.saving || this.#closed) return;
        if (!back && detail.updateAt <= this.#readAt) return;
        this.#takeDetail(detail);
      })
      .catch(() => {
        // The next change, or the next write, will tell.
      });
  }

  // ------------------------------------------- comments and attachments

  /** A comment or an attachment arrived from the backend. */
  receive(block: Comment | Attachment): void {
    if (block.parentId !== this.cardId) return;
    if (block.type === 'comment') this.#comments = take(this.#comments, block);
    else this.#attachments = take(this.#attachments, block);
  }

  /**
   * Reads the comments and attachments again, after changes may have been
   * missed (the board's `resync`), merged with what is shown. The body is
   * the board's to read again (`cardChanged`), which minds what the reader
   * has not saved. A card that is gone is missing.
   */
  async refresh(): Promise<void> {
    if (this.status !== 'ready' || this.#closed) return;
    this.#refreshing += 1;
    const reading = this.#refreshing;
    const commentsBefore = this.#comments.map((comment) => comment.id);
    const attachmentsBefore = this.#attachments.map((attachment) => attachment.id);
    try {
      const [comments, attachments] = await Promise.all([
        this.#backend.comments(this.#boardId, this.cardId),
        this.#backend.attachments(this.#boardId, this.cardId),
      ]);
      // Closed meanwhile, or a later reading has begun (asked again: the
      // check above was before the wait, which TypeScript does not see).
      if (this.#isClosed() || reading !== this.#refreshing) return;
      this.#comments = reread(this.#comments, comments, commentsBefore);
      this.#attachments = reread(this.#attachments, attachments, attachmentsBefore);
    } catch (error) {
      if (this.#isClosed() || reading !== this.#refreshing) return;
      if (isBackendError(error) && error.status === 404) this.missing = true;
      // Otherwise the next change, or the next opening, brings them.
    }
  }

  #isClosed(): boolean {
    return this.#closed;
  }

  /** Blocks were deleted; they may be this card or things of it. */
  forget(blockIds: readonly string[]): void {
    if (blockIds.includes(this.cardId)) this.missing = true;
    this.#comments = this.#comments.filter((comment) => !blockIds.includes(comment.id));
    this.#attachments = this.#attachments.filter((attachment) => !blockIds.includes(attachment.id));
  }

  async addComment(body: DocNode): Promise<boolean> {
    try {
      this.receive(await this.#backend.createComment(this.#boardId, this.cardId, body));
      return true;
    } catch (error) {
      this.board.workspace.report('save', error);
      return false;
    }
  }

  async #deleteChild(blockId: string, description: string): Promise<void> {
    const comment = this.#comments.find((existing) => existing.id === blockId);
    const attachment = this.#attachments.find((existing) => existing.id === blockId);
    try {
      await this.board.workspace.undo.perform(
        async () => {
          await this.#backend.deleteBlock(this.#boardId, blockId);
          this.forget([blockId]);
        },
        async () => {
          await this.#backend.restoreBlock(this.#boardId, blockId);
          if (comment) this.receive(comment);
          if (attachment) this.receive(attachment);
        },
        description,
      );
    } catch (error) {
      this.board.workspace.report('save', error);
    }
  }

  async deleteComment(commentId: string): Promise<void> {
    await this.#deleteChild(commentId, 'deleteComment');
  }

  async deleteAttachment(attachmentId: string): Promise<void> {
    await this.#deleteChild(attachmentId, 'deleteAttachment');
  }

  /** Uploads a file, showing its progress in `uploads`. Undefined when it failed or was given up. */
  async #upload(
    file: File,
  ): Promise<{ fileId: string; name: string; size: number; contentType: string } | undefined> {
    const controller = new AbortController();
    this.#uploadCount += 1;
    const id = this.#uploadCount;
    const update = (change: Partial<Upload>): void => {
      this.uploads = this.uploads.map((upload) =>
        upload.id === id ? { ...upload, ...change } : upload,
      );
    };
    const remove = (): void => {
      this.uploads = this.uploads.filter((upload) => upload.id !== id);
    };
    this.uploads = [
      ...this.uploads,
      {
        id,
        name: file.name,
        sentBytes: 0,
        totalBytes: file.size,
        failed: false,
        cancel: () => {
          controller.abort();
          remove();
        },
      },
    ];
    try {
      const uploaded = await this.#backend.upload(this.#boardId, file, {
        signal: controller.signal,
        onProgress: ({ sentBytes, totalBytes }) => {
          update({ sentBytes, totalBytes });
        },
      });
      remove();
      return uploaded;
    } catch (error) {
      if (controller.signal.aborted) return undefined;
      // The row stays, marked as failed, until the reader dismisses it.
      update({ failed: true });
      this.board.workspace.report('upload', error);
      return undefined;
    }
  }

  /** Uploads a file and attaches it to the card. */
  async attach(file: File): Promise<void> {
    const uploaded = await this.#upload(file);
    if (!uploaded) return;
    try {
      this.receive(await this.#backend.createAttachment(this.#boardId, this.cardId, uploaded));
    } catch (error) {
      this.board.workspace.report('save', error);
    }
  }

  /** Uploads an image for the body and gives the identifier an image node names. */
  async uploadImage(file: File): Promise<string> {
    const uploaded = await this.#upload(file);
    if (!uploaded) throw new Error('the image was not uploaded');
    return uploaded.fileId;
  }

  /** Where the bytes of a file of this board are. */
  fileUrl(fileId: string): string | undefined {
    return this.#backend.fileUrl(this.#boardId, fileId);
  }
}
