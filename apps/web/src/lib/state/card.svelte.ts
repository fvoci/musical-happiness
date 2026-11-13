/**
 * One open card: its body, comments and attachments.
 *
 * The title, icon and properties of the card are not here: they are in the
 * card list of the board store (`summary`), so that the card and the board
 * behind it never disagree.
 *
 * The body is edited together in the card's room (`openBody`,
 * `Backend.openDoc`): the room's document is what the reader sees and
 * writes, and the server writes the card's body from it. `body` is that body
 * as the card was last read, shown until the room's document has arrived,
 * and where there is no room (a reader by a public link). While the room's
 * document is held, a change of the card does not read the body again: the
 * room's is the newer.
 */
import type { DocNode } from '../editor/schema.ts';

import {
  isBackendError,
  isConflict,
  type Attachment,
  type BodyDoc,
  type BodyDocState,
  type CardDetail,
  type CardSummary,
  type Comment,
} from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import { byCreation } from './order.ts';
import type { LoadStatus } from './workspace.svelte.ts';
import { reread, take } from './write.ts';

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

/** How long the line at the top of the card says "saving" after the server had it all. */
export const SAVING_SHOWN = 600;

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
  /** The body in its room, while it is open there (`openBody`). */
  room = $state.raw<BodyDocState | undefined>(undefined);
  /**
   * Why changes of the reader were last dropped with the document they were
   * in (a reason of the server's, or `document_reset`), until it is said and
   * dismissed (`dismissLost`).
   */
  lost = $state<string | undefined>(undefined);
  /** The reader wrote in the body in this visit. */
  bodyEdited = $state(false);
  /**
   * What the reader wrote is on its way, or reached the server less than
   * `SAVING_SHOWN` ago: the line that says so does not blink at every key.
   */
  bodySaving = $state(false);
  uploads = $state.raw<Upload[]>([]);

  #comments = $state.raw<Comment[]>([]);
  #attachments = $state.raw<Attachment[]>([]);
  /** Oldest first. */
  readonly comments = $derived(byCreation(this.#comments));
  readonly attachments = $derived(byCreation(this.#attachments));

  /** Title, icon and properties, from the board's list. */
  readonly summary = $derived.by<CardSummary | undefined>(() => this.board.card(this.cardId));

  /** The version of the card the body was read at. */
  #readAt = 0;
  #bodyDoc: BodyDoc | undefined;
  #savingShown: ReturnType<typeof setTimeout> | undefined;
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
    this.closeBody();
    for (const upload of this.uploads) upload.cancel();
    for (const upload of this.#imageUploads) upload.abort();
  }

  #takeDetail(detail: CardDetail): void {
    const { body, ...summary } = detail;
    this.#readAt = detail.updateAt;
    this.missing = false;
    this.board.receiveCard(summary);
    if (!sameDoc(body, this.body)) this.body = body;
  }

  // --------------------------------------------------------------- body

  /**
   * Opens the body in its room, unless it is open there or the backend has
   * no rooms. What the room says is in `room`; closing the card leaves it.
   */
  openBody(): void {
    const backend = this.#backend;
    if (this.#bodyDoc || this.#closed || !backend.openDoc) return;
    const workspaceId = this.board.workspace.info?.id ?? '';
    const opened = backend.openDoc(workspaceId, this.#boardId, this.cardId, {
      state: (state) => {
        if (this.#bodyDoc !== opened) return;
        this.room = state;
        this.#showSaving(state.pending);
        // Without the room's document the body is the card's again: read what was missed.
        if (!state.doc) this.#catchUp();
      },
      discarded: (why) => {
        if (this.#bodyDoc === opened) this.lost = why;
      },
    });
    this.#bodyDoc = opened;
    this.room = opened.state;
  }

  #showSaving(pending: boolean): void {
    clearTimeout(this.#savingShown);
    if (pending) {
      this.bodyEdited = true;
      this.bodySaving = true;
      return;
    }
    this.#savingShown = setTimeout(() => {
      this.bodySaving = false;
    }, SAVING_SHOWN);
  }

  /** Leaves the body's room; what waits is sent first. */
  closeBody(): void {
    clearTimeout(this.#savingShown);
    const opened = this.#bodyDoc;
    this.#bodyDoc = undefined;
    this.room = undefined;
    opened?.close();
  }

  /** Sends what the reader wrote at once; settles when the server has it, or cannot now. */
  settleBody(): Promise<void> {
    return this.#bodyDoc?.settle() ?? Promise.resolve();
  }

  /** Asks to be let into the room again now. */
  retryBody(): void {
    this.#bodyDoc?.retry();
  }

  /** The reader was told that changes were dropped. */
  dismissLost(): void {
    this.lost = undefined;
  }

  /** What the reader wrote has not all reached the server. */
  get bodyPending(): boolean {
    return this.room?.pending ?? false;
  }

  /**
   * The card changed in the board's list. If that is news, and the body is
   * not the room's now, it is read again so that somebody else's writing
   * shows where the room's document is not.
   */
  cardChanged(card: CardSummary): void {
    // A card that was deleted and is back (restored) is read again whatever its time.
    const back = this.missing;
    if (card.id !== this.cardId || (!back && card.updateAt <= this.#readAt)) return;
    // A card back from the trash is read whatever the room holds: it was missing.
    if (this.#busy() && !back) {
      this.#news = card;
      return;
    }
    void this.#backend
      .card(this.#boardId, this.cardId)
      .then((detail) => {
        if (this.#busy() && !back) {
          this.#news = card;
          return;
        }
        if (!back && detail.updateAt <= this.#readAt) return;
        this.#takeDetail(detail);
      })
      .catch(() => {
        // The next change, or the next write, will tell.
      });
  }

  /**
   * The latest news of the card heard while the body was the room's
   * (`#busy`): it is read once the room's document is gone (`#catchUp`).
   */
  #news: CardSummary | undefined;

  /** The body shown is the room's document, or the card is closed. */
  #busy(): boolean {
    return this.#closed || this.room?.doc !== undefined;
  }

  #catchUp(): void {
    const news = this.#news;
    if (!news || this.#busy()) return;
    this.#news = undefined;
    this.cardChanged(news);
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

  /**
   * Changes the text of a comment, written over the version `readAt` (the
   * one the reader began from). When somebody changed it since (409) nothing
   * is overwritten: the comments are read again, so that the caller can show
   * theirs, and the reader's text stays with the caller to be saved again.
   */
  async editComment(commentId: string, body: DocNode, readAt: number): Promise<CommentEdit> {
    try {
      this.receive(await this.#backend.patchComment(this.#boardId, commentId, { body, readAt }));
      return 'saved';
    } catch (error) {
      if (!isConflict(error)) {
        this.board.workspace.report('save', error);
        return 'failed';
      }
      await this.refresh();
      return 'conflict';
    }
  }

  /** A comment as this store has it now. */
  comment(commentId: string): Comment | undefined {
    return this.#comments.find((existing) => existing.id === commentId);
  }

  async #deleteChild(blockId: string, description: string): Promise<void> {
    const comment = this.#comments.find((existing) => existing.id === blockId);
    const child = comment ?? this.#attachments.find((existing) => existing.id === blockId);
    try {
      await this.board.workspace.undo.perform(
        async () => {
          this.board.deleting(blockId, comment ? this.cardId : undefined);
          await this.#backend.deleteBlock(this.#boardId, blockId);
          this.forget([blockId]);
        },
        async () => {
          await this.#backend.restoreBlock(this.#boardId, blockId);
          if (child) this.receive(child);
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

  /**
   * Uploads an image for the body and gives the identifier an image node
   * names. The editor shows its place meanwhile (not the list of uploads);
   * the place follows the room's document, whatever others write meanwhile.
   */
  async uploadImage(file: File): Promise<string> {
    const controller = new AbortController();
    this.#imageUploads = [...this.#imageUploads, controller];
    try {
      const uploaded = await this.#backend.upload(this.#boardId, file, {
        signal: controller.signal,
      });
      return uploaded.fileId;
    } catch (error) {
      if (!controller.signal.aborted) this.board.workspace.report('upload', error);
      throw error;
    } finally {
      this.#imageUploads = this.#imageUploads.filter((other) => other !== controller);
    }
  }

  /** The uploads of images for the body under way. */
  #imageUploads = $state.raw<AbortController[]>([]);

  /** An image is going up for the body: closing the card now would give it up. */
  get imagesGoingUp(): boolean {
    return this.#imageUploads.length > 0;
  }

  /** The bytes of a file of this board; undefined (and told) when they could not be read. */
  async fileBytes(fileId: string): Promise<Blob | undefined> {
    try {
      return await this.#backend.fileBytes(this.#boardId, fileId);
    } catch (error) {
      this.board.workspace.report('load', error);
      return undefined;
    }
  }

  /** Where the bytes of a file of this board are. */
  fileUrl(fileId: string): string | undefined {
    return this.#backend.fileUrl(this.#boardId, fileId);
  }
}
