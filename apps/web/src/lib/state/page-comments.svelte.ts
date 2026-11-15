/**
 * The comments on the page shown, as a card's are on an open card
 * (`card.svelte.ts`): read with the page, oldest first, and those that
 * others write, change and delete arriving as the wiki hears them
 * (`WikiStore`, which hands them here). A comment is written, changed over
 * the text it was read at (409: somebody changed it since, and theirs is
 * read again for the caller to show) and deleted by whom the wiki lets, as
 * a board lets of a card's (`canEditComment`, the wiki's role). A comment
 * deleted here comes back with "Undo", as a card's does.
 */
import type { DocNode } from '../editor/schema.ts';
import { isBackendError, isConflict, type Comment, type CommentVersion } from './backend.ts';
import { canChangeComment } from './board-parts.ts';
import type { CommentEdit } from './card-parts.ts';
import type { LoadStatus } from './workspace.svelte.ts';
import { byCreation } from './order.ts';
import { authorIds } from './user-names.ts';
import type { WikiStore } from './wiki.svelte.ts';
import type { Wikis } from './wiki-types.ts';
import { reread, take } from './write.ts';

export class PageComments {
  readonly #wiki: WikiStore;
  readonly #pageId: string;
  #comments = $state.raw<Comment[]>([]);
  /** Oldest first. */
  readonly comments = $derived(byCreation(this.#comments));
  /** Whether they have been read: until then there are none to say there are none. */
  status = $state<LoadStatus>('idle');
  /**
   * The comments deleted while this store was open: a reading answered before
   * a deletion it crossed does not bring one back; "Undo" does, and takes it
   * off this list.
   */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- consulted, never shown
  readonly #deleted = new Set<string>();
  /** Readings begun: the latest is the one that counts. */
  #readings = 0;
  #closed = false;

  constructor(wiki: WikiStore, pageId: string) {
    this.#wiki = wiki;
    this.#pageId = pageId;
  }

  get #wikis(): Wikis {
    const wikis = this.#wiki.workspace.backend.wiki;
    if (!wikis) throw new Error('this backend has no wikis');
    return wikis;
  }

  get #wikiId(): string {
    return this.#wiki.wikiId;
  }

  // ------------------------------------------------- who may do what

  get workspace(): WikiStore['workspace'] {
    return this.#wiki.workspace;
  }

  /** Whether the reader writes comments: the wiki's commenters and up. */
  get canComment(): boolean {
    return this.#wiki.canEdit || this.#wiki.role === 'commenter';
  }

  /** Whether the reader changes the text of a comment `createdBy` wrote ([`canChangeComment`]). */
  canEditComment(createdBy: string): boolean {
    return canChangeComment(this.#wiki.role, createdBy, this.#wiki.workspace.me?.id);
  }

  /** Whether the reader deletes a comment `createdBy` wrote: as `canEditComment`. */
  canDeleteComment(createdBy: string): boolean {
    return this.canEditComment(createdBy);
  }

  /** Whether the reader reads what comments said before: any member of the wiki. */
  get canReadCommentHistory(): boolean {
    return this.#wiki.role !== '';
  }

  /**
   * Reads the comments (again), merged with what is shown. A page that is
   * not there is the page's to say (`PageStore`); another failure is told
   * on the first reading only, and the next change brings them.
   */
  async read(first: boolean): Promise<void> {
    this.#readings += 1;
    const reading = this.#readings;
    const before = this.#comments.map((comment) => comment.id);
    // Deleted before the reading began, a comment it finds was brought back since.
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- consulted, never shown
    const deletedBefore = new Set(this.#deleted);
    if (first || this.status === 'failed') this.status = 'loading';
    try {
      const read = await this.#wikis.comments(this.#wikiId, this.#pageId);
      if (this.#closed || reading !== this.#readings) return;
      for (const comment of read)
        if (deletedBefore.has(comment.id)) this.#deleted.delete(comment.id);
      const comments = read.filter((comment) => !this.#deleted.has(comment.id));
      void this.#wiki.workspace.learnUsers(authorIds(comments));
      this.#comments = reread(this.#comments, comments, before);
      this.status = 'ready';
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      if (this.status !== 'ready') this.status = 'failed';
      const gone = isBackendError(error) && (error.status === 404 || error.status === 403);
      if (first && !gone) this.#wiki.workspace.report('load', error);
    }
  }

  close(): void {
    this.#closed = true;
  }

  /**
   * A comment the wiki heard of: one on this page is taken in, also one that
   * was deleted, which was brought back since.
   */
  receive(comment: Comment): void {
    if (comment.parentId !== this.#pageId || this.#closed) return;
    this.#deleted.delete(comment.id);
    void this.#wiki.workspace.learnUsers(authorIds([comment]));
    this.#comments = take(this.#comments, comment);
  }

  /** The answer to the reader's own write: not of a comment deleted meanwhile. */
  #answered(comment: Comment): void {
    if (!this.#deleted.has(comment.id)) this.receive(comment);
  }

  /** Blocks were deleted; some may be comments of this page. */
  forget(blockIds: readonly string[]): void {
    for (const id of blockIds) this.#deleted.add(id);
    this.#comments = this.#comments.filter((comment) => !blockIds.includes(comment.id));
  }

  /** A comment as this store has it now. */
  comment(commentId: string): Comment | undefined {
    return this.#comments.find((existing) => existing.id === commentId);
  }

  async addComment(body: DocNode): Promise<boolean> {
    try {
      this.#answered(await this.#wikis.createComment(this.#wikiId, this.#pageId, body));
      return true;
    } catch (error) {
      this.#wiki.workspace.report('save', error);
      return false;
    }
  }

  /** As `CardStore.editComment`: written over the version `readAt`, or read again on a conflict. */
  async editComment(commentId: string, body: DocNode, readAt: number): Promise<CommentEdit> {
    try {
      const write = { body, readAt };
      this.#answered(await this.#wikis.patchComment(this.#wikiId, this.#pageId, commentId, write));
      return 'saved';
    } catch (error) {
      if (!isConflict(error)) {
        this.#wiki.workspace.report('save', error);
        return 'failed';
      }
      await this.read(false);
      return 'conflict';
    }
  }

  /** Deletes a comment, which "Undo" brings back onto the page, as a card's. */
  async deleteComment(commentId: string): Promise<void> {
    const [wikiId, pageId] = [this.#wikiId, this.#pageId];
    try {
      await this.#wiki.workspace.undo.perform(
        async () => {
          await this.#wikis.deleteComment(wikiId, pageId, commentId);
          this.forget([commentId]);
        },
        async () => {
          this.receive(await this.#wikis.restoreComment(wikiId, pageId, commentId));
        },
        'deleteComment',
      );
    } catch (error) {
      this.#wiki.workspace.report('save', error);
    }
  }

  /** What a comment said before, with its writers known by name. */
  async commentVersions(commentId: string): Promise<CommentVersion[]> {
    const list = await this.#wikis.commentVersions(this.#wikiId, this.#pageId, commentId);
    await this.#wiki.workspace.learnUsers(
      list.map((version) => version.writtenBy).filter((id) => id !== ''),
    );
    return list;
  }
}
