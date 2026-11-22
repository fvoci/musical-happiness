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
import { BodyUploads } from './body-uploads.svelte.ts';
import type { BoardStore } from './board.svelte.ts';
import { REJOIN_SPACING, afterPutOut, type CommentEdit } from './card-parts.ts';
import { byCreation } from './order.ts';
import { authorIds } from './user-names.ts';
import type { LoadStatus } from './workspace.svelte.ts';
import { UploadKept, type Transfer } from './transfers.svelte.ts';
import { reread, take } from './write.ts';

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
   * Why things of the reader's were dropped (a reason of the server's,
   * `document_reset`, or what the editor could not put in), each once, until
   * they are said and dismissed (`dismissLost`): none replaces another.
   */
  lost = $state.raw<readonly string[]>([]);
  /** The reader wrote in the body in this visit. */
  bodyEdited = $state(false);
  /**
   * What the reader wrote is on its way, or reached the server less than
   * `SAVING_SHOWN` ago: the line that says so does not blink at every key.
   */
  bodySaving = $state(false);
  /**
   * The files going up to be attached here, or that failed to, as the stack of
   * the workspace has them: they outlive the card. One that is in is an attachment,
   * one given up too late as well (`kept`).
   */
  readonly uploads = $derived.by<Transfer[]>(() =>
    this.board.workspace.transfers.list.filter(
      (one) => one.card === this.cardId && !one.finished && one.failure !== 'kept',
    ),
  );

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
  #closed = false;
  /** When the room was last joined again by itself (`#stillThere`). */
  #rejoinedAt = -Infinity;
  /** Has the board read again: a card gone for the reader may take its board along. */
  readonly #readBoardAgain: () => void;

  constructor(board: BoardStore, cardId: string, readBoardAgain: () => void) {
    this.board = board;
    this.cardId = cardId;
    this.#readBoardAgain = readBoardAgain;
  }

  get #backend(): BoardStore['workspace']['backend'] {
    return this.board.workspace.backend;
  }

  get #boardId(): string {
    return this.board.boardId;
  }

  /**
   * Reads the card's body, comments and attachments, and asks for the body's
   * room meanwhile, so that its document comes with them. One that cannot be
   * read leaves the room again: nothing shows the body of a card not read.
   */
  async load(): Promise<void> {
    this.status = 'loading';
    this.openBody();
    try {
      const [detail, comments, attachments] = await Promise.all([
        this.#backend.card(this.#boardId, this.cardId),
        this.#backend.comments(this.#boardId, this.cardId),
        this.#backend.attachments(this.#boardId, this.cardId),
      ]);
      if (this.#closed) return;
      this.#takeDetail(detail);
      void this.board.workspace.learnUsers(authorIds([detail, ...comments]));
      this.#comments = comments;
      this.#attachments = attachments;
      this.status = 'ready';
    } catch (error) {
      this.status = 'failed';
      this.closeBody();
      if (isBackendError(error) && error.status === 404) this.missing = true;
      else this.board.workspace.report('load', error);
    }
  }

  close(): void {
    this.#closed = true;
    this.closeBody();
    this.#bodyUploads.giveUp();
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
   * Opens the body in its room, unless it is open there, the card is
   * missing, or the backend has no rooms. What the room says is in `room`;
   * closing the card leaves it. It is asked for with the card's first
   * reading (`load`), which decides whether the card is there.
   */
  openBody(): void {
    const backend = this.#backend;
    // Not before the workspace is known: the room is asked for in it (opened again once it is).
    const workspaceId = this.board.workspace.info?.id;
    const notNow = this.#bodyDoc !== undefined || this.#closed || this.missing;
    if (notNow || !backend.openDoc || workspaceId === undefined) return;
    const opened = backend.openDoc(workspaceId, this.#boardId, this.cardId, {
      state: (state) => {
        if (this.#bodyDoc !== opened) return;
        const shut = state.status === 'out' && state.reason === 'not_found';
        const heldDoc = this.room?.doc !== undefined;
        // While the card is first read, that reading says whether it is there (`load`).
        const asked = shut && this.room?.reason !== 'not_found' && this.status !== 'loading';
        if (asked) void this.#stillThere(opened, heldDoc);
        this.room = state;
        this.#showSaving(state.pending);
        // Without the room's document the body is the card's again: read what was missed.
        if (!state.doc) this.#catchUp();
      },
      discarded: (why) => {
        if (this.#bodyDoc === opened) this.sayLost(why);
      },
    });
    this.#bodyDoc = opened;
    this.room = opened.state;
  }

  /**
   * The room let the reader go as it does for a card that is not there. The
   * card says why (`afterPutOut`). Gone for the reader, it is missing, and the
   * board is read again: one that is gone for them takes the card's window
   * along. Still there (restored before the room told), a room that gave the
   * reader its document is joined again, not sooner than `REJOIN_SPACING`
   * after the last time; one that refused them is not asked again.
   */
  async #stillThere(opened: BodyDoc, heldDoc: boolean): Promise<void> {
    const read = await afterPutOut(() => this.#backend.card(this.#boardId, this.cardId));
    // Of a room that is still this card's: not one left meanwhile, or of the card restored.
    if (this.#bodyDoc !== opened) return;
    if (read.found === 'there' && heldDoc && Date.now() - this.#rejoinedAt >= REJOIN_SPACING) {
      this.#rejoinedAt = Date.now();
      opened.retry();
    }
    if (read.found === 'ended') this.board.workspace.report('load', read.error);
    if (read.found !== 'gone') return;
    this.missing = true;
    this.#readBoardAgain();
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
    this.bodySaving = false;
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

  /** Something of the reader's that was not put into the body is said as lost, until dismissed. */
  sayLost(why: string): void {
    if (!this.lost.includes(why)) this.lost = [...this.lost, why];
  }

  /** The reader was told that changes were dropped. */
  dismissLost(): void {
    this.lost = [];
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
    void this.board.workspace.learnUsers(authorIds([block]));
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
      void this.board.workspace.learnUsers(authorIds(comments));
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

  /**
   * Uploads a file and attaches it to the card, shown in the stack of the
   * workspace (`uploads` here). Closing the card does not give it up: the card
   * open then takes the attachment. One given up as it was being attached
   * is taken away again; where that fails, it stays attached, and the stack
   * says so (`UploadKept`).
   */
  async attach(file: File): Promise<void> {
    const { workspace, boardId } = this.board;
    const { backend } = workspace;
    const cardId = this.cardId;
    const shown = (attachment: Attachment): void => {
      const open = workspace.board?.openCard;
      if (open?.cardId === cardId && open.board.boardId === boardId) open.receive(attachment);
    };
    await workspace.transfers.upload({
      file,
      card: cardId,
      retryable: true,
      async send(options) {
        const uploaded = await backend.upload(boardId, file, options);
        options.signal.throwIfAborted();
        const attachment = await backend.createAttachment(boardId, cardId, uploaded);
        if (!options.signal.aborted) {
          shown(attachment);
          return;
        }
        try {
          await backend.deleteBlock(boardId, attachment.id);
        } catch (error) {
          // Gone already: nothing stays.
          if (isBackendError(error) && error.status === 404) return;
          shown(attachment);
          throw new UploadKept(error);
        }
      },
    }).done;
  }

  /**
   * Uploads a file for the body and gives the identifier its image or file block
   * names. The editor shows its place meanwhile, which follows the room's
   * document whatever others write; the stack shows how far it is. Closing
   * the card gives it up.
   */
  uploadFile(file: File): Promise<string> {
    return this.#bodyUploads.run(file, (options) =>
      this.#backend.upload(this.#boardId, file, options),
    );
  }

  /** The uploads of files for the body under way. */
  readonly #bodyUploads = new BodyUploads(() => this.board.workspace);

  /** A file is going up for the body: closing the card now would give it up. */
  get filesGoingUp(): boolean {
    return this.#bodyUploads.goingUp;
  }

  /** A file's bytes; undefined when not read (told), or given up by `signal` (not told). */
  async fileBytes(fileId: string, signal?: AbortSignal): Promise<Blob | undefined> {
    try {
      return await this.#backend.fileBytes(this.#boardId, fileId, signal);
    } catch (error) {
      if (!signal?.aborted) this.board.workspace.report('load', error);
      return undefined;
    }
  }

  /** Where the bytes of a file of this board are. */
  fileUrl(fileId: string): string | undefined {
    return this.#backend.fileUrl(this.#boardId, fileId);
  }
}
