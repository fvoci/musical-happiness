/**
 * The comments on the unit shown, as a wiki page's are (`page-comments.svelte.ts`):
 * read with the unit, oldest first, with live updates forwarded from `CourseStore`.
 */
import type { DocNode } from '../editor/schema.ts';

import { isBackendError, isConflict, type Comment, type CommentVersion } from './backend.ts';
import { canChangeComment } from './board-parts.ts';
import type { CommentEdit } from './card-parts.ts';
import type { Courses } from './course-types.ts';
import type { CourseStore } from './course.svelte.ts';
import { byCreation } from './order.ts';
import { authorIds } from './user-names.ts';
import type { LoadStatus } from './workspace.svelte.ts';
import { reread, take } from './write.ts';

export class UnitComments {
  readonly #course: CourseStore;
  readonly #unitId: string;
  readonly #onCommented: (() => void) | undefined;
  #comments = $state.raw<Comment[]>([]);
  /** Oldest first. */
  readonly comments = $derived(byCreation(this.#comments));
  status = $state<LoadStatus>('idle');
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- consulted, never shown
  readonly #deleted = new Map<string, number>();
  #readings = 0;
  #closed = false;

  constructor(course: CourseStore, unitId: string, onCommented?: () => void) {
    this.#course = course;
    this.#unitId = unitId;
    this.#onCommented = onCommented;
  }

  get #courses(): Courses {
    const courses = this.#course.workspace.backend.course;
    if (!courses) throw new Error('this backend has no courses');
    return courses;
  }

  get #courseId(): string {
    return this.#course.courseId;
  }

  get workspace(): CourseStore['workspace'] {
    return this.#course.workspace;
  }

  /** Whether the reader writes comments: the course's commenters and up. */
  get canComment(): boolean {
    return this.#course.canEdit || this.#course.role === 'commenter';
  }

  canEditComment(createdBy: string): boolean {
    return canChangeComment(this.#course.role, createdBy, this.#course.workspace.me?.id);
  }

  canDeleteComment(createdBy: string): boolean {
    return this.canEditComment(createdBy);
  }

  /** Any member of the course, and of an open one whoever works in the workspace (`joinable`). */
  get canReadCommentHistory(): boolean {
    return this.#course.role !== '' || this.#course.joinable;
  }

  async read(first: boolean): Promise<void> {
    this.#readings += 1;
    const reading = this.#readings;
    const before = this.#comments.map((comment) => comment.id);
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- consulted, never shown
    const deletedBefore = new Map(this.#deleted);
    if (first || this.status === 'failed') this.status = 'loading';
    try {
      const read = await this.#courses.comments(this.#courseId, this.#unitId);
      if (this.#closed || reading !== this.#readings) return;
      for (const comment of read) {
        if (deletedBefore.get(comment.id) === this.#deleted.get(comment.id)) {
          this.#deleted.delete(comment.id);
        }
      }
      const comments = read.filter((comment) => !this.#deleted.has(comment.id));
      void this.#course.workspace.learnUsers(authorIds(comments));
      this.#comments = reread(this.#comments, comments, before);
      this.status = 'ready';
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      if (isBackendError(error) && error.status === 401) {
        this.#course.workspace.report('load', error);
        return;
      }
      if (this.status !== 'ready') this.status = 'failed';
      const gone = isBackendError(error) && (error.status === 404 || error.status === 403);
      if (first && !gone) this.#course.workspace.report('load', error);
    }
  }

  close(): void {
    this.#closed = true;
  }

  receive(comment: Comment): void {
    if (comment.parentId !== this.#unitId || this.#closed) return;
    if (this.#deleted.has(comment.id)) {
      void this.read(false);
      return;
    }
    this.#take(comment);
  }

  #take(comment: Comment): void {
    void this.#course.workspace.learnUsers(authorIds([comment]));
    this.#comments = take(this.#comments, comment);
  }

  #answered(comment: Comment): void {
    if (comment.parentId === this.#unitId && !this.#closed && !this.#deleted.has(comment.id)) {
      this.#take(comment);
    }
  }

  forget(blockIds: readonly string[]): void {
    for (const id of blockIds) this.#deleted.set(id, (this.#deleted.get(id) ?? 0) + 1);
    this.#comments = this.#comments.filter((comment) => !blockIds.includes(comment.id));
  }

  comment(commentId: string): Comment | undefined {
    return this.#comments.find((existing) => existing.id === commentId);
  }

  async addComment(body: DocNode): Promise<boolean> {
    try {
      this.#answered(await this.#courses.createComment(this.#courseId, this.#unitId, body));
      this.#onCommented?.();
      return true;
    } catch (error) {
      this.#course.workspace.report('save', error);
      return false;
    }
  }

  async editComment(commentId: string, body: DocNode, readAt: number): Promise<CommentEdit> {
    try {
      const write = { body, readAt };
      this.#answered(
        await this.#courses.patchComment(this.#courseId, this.#unitId, commentId, write),
      );
      return 'saved';
    } catch (error) {
      if (!isConflict(error)) {
        this.#course.workspace.report('save', error);
        return 'failed';
      }
      await this.read(false);
      return 'conflict';
    }
  }

  async deleteComment(commentId: string): Promise<void> {
    const [courseId, unitId] = [this.#courseId, this.#unitId];
    try {
      await this.#course.workspace.undo.perform(
        async () => {
          await this.#courses.deleteComment(courseId, unitId, commentId);
          this.forget([commentId]);
        },
        async () => {
          this.#deleted.delete(commentId);
          this.#answered(await this.#courses.restoreComment(courseId, unitId, commentId));
        },
        'deleteComment',
      );
    } catch (error) {
      this.#course.workspace.report('save', error);
    }
  }

  async commentVersions(commentId: string): Promise<CommentVersion[]> {
    const list = await this.#courses.commentVersions(this.#courseId, this.#unitId, commentId);
    await this.#course.workspace.learnUsers(
      list.map((version) => version.writtenBy).filter((id) => id !== ''),
    );
    return list;
  }
}
