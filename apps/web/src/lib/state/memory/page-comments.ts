/**
 * The comments on the pages of the memory backend's wikis, as the server has
 * them (`getPageComments` and the rest in `contracts/openapi.json`): a
 * comment is a block of the type `comment` below its page, and who may do
 * what to one is what a card's comments let (`comments.ts`), by the role on
 * the wiki. The role is asked first, then the page, then the comment: who
 * may not do it learns nothing of what is there.
 *
 * `MemoryPageComments` is what `MemoryWikis` is besides its pages.
 */
import type { DocNode } from '../../editor/schema.ts';
import type { BodyWrite, Comment, CommentVersion } from '../backend.ts';
import type { Wikis } from '../wiki-types.ts';

import { memberRole } from './board-access.ts';
import { checkedBody } from './bodies.ts';
import { earlierVersions, mayChangeComment, mayDeleteComment, noteChange } from './comments.ts';
import { fail } from './refuse.ts';
import { commentOf, scramble } from './shapes.ts';
import { deletion } from './trash.ts';
import type { StoredBlock, World } from './world.ts';

/** Who asks, in which world, about the comments of which page. */
export interface OnPage {
  readonly world: World;
  readonly userId: string;
  readonly wikiId: string;
  readonly pageId: string;
}

/**
 * The role on the wiki, which is to be one to comment with; refused (403) for
 * a reader, in the server's words: `denial` for a change of a comment.
 */
function commenterRole(
  on: OnPage,
  denial = 'access denied to comment on the page',
): 'admin' | 'editor' | 'commenter' {
  const role = memberRole(on.world, on.userId, on.wikiId, 'wiki');
  if (role === 'viewer') fail(403, denial);
  return role;
}

/** What the server refuses a reader's change of a comment with. */
const CHANGES_DENIED = 'access denied to change the comments of the page';

/** The page, a page of the wiki and not in the trash: 404 otherwise. */
function pageOn(on: OnPage): StoredBlock {
  const page = on.world.blocks.get(on.pageId);
  if (page?.type !== 'page' || page.boardId !== on.wikiId) fail(404, 'no such page');
  return page;
}

/** A comment on the page; 404 for any other block, or none. */
function commentOn(on: OnPage, commentId: string): StoredBlock {
  const comment = on.world.blocks.get(commentId);
  if (comment?.type !== 'comment' || comment.parentId !== on.pageId) {
    fail(404, 'no such comment');
  }
  return comment;
}

function tell(on: OnPage, comment: StoredBlock): void {
  on.world.tellBoard(on.wikiId, {
    kind: 'blocks',
    boardId: on.wikiId,
    blocks: [commentOf(comment)],
  });
}

export function commentsOf(on: OnPage): Comment[] {
  memberRole(on.world, on.userId, on.wikiId, 'wiki');
  pageOn(on);
  const comments = on.world.childrenOf(on.pageId).filter((block) => block.type === 'comment');
  // In no order to count on, as every list of this backend: the screens sort them.
  return scramble(comments).map(commentOf);
}

export function createComment(on: OnPage, body: DocNode): Comment {
  commenterRole(on);
  pageOn(on);
  const text = checkedBody(on.world, on.wikiId, body);
  if (text === null) fail(400, 'a comment has a text', { field: 'body' });
  const now = on.world.now();
  const comment: StoredBlock = {
    id: on.world.newId(),
    boardId: on.wikiId,
    parentId: on.pageId,
    createdBy: on.userId,
    modifiedBy: on.userId,
    schema: 1,
    type: 'comment',
    title: '',
    fields: {},
    body: text,
    createAt: now,
    updateAt: now,
    deleteAt: 0,
  };
  on.world.blocks.set(comment.id, comment);
  tell(on, comment);
  return commentOf(comment);
}

export function patchComment(on: OnPage, commentId: string, write: BodyWrite): Comment {
  const role = commenterRole(on, CHANGES_DENIED);
  pageOn(on);
  const comment = commentOn(on, commentId);
  if (!mayChangeComment(role, comment.createdBy === on.userId)) {
    fail(403, "access denied to change other users' comments");
  }
  const text = checkedBody(on.world, on.wikiId, write.body);
  if (text === null) fail(400, 'a comment has a text', { field: 'body' });
  if (comment.updateAt !== write.readAt) fail(409, 'the comment changed since it was read');
  const before = comment.body ?? null;
  comment.body = text;
  comment.updateAt = Math.max(on.world.now(), comment.updateAt + 1);
  comment.modifiedBy = on.userId;
  noteChange(on.world, comment, before, on.userId);
  tell(on, comment);
  return commentOf(comment);
}

export function deleteComment(on: OnPage, commentId: string): void {
  const role = commenterRole(on, CHANGES_DENIED);
  pageOn(on);
  const comment = commentOn(on, commentId);
  if (!mayDeleteComment(role, comment.createdBy === on.userId)) {
    fail(403, "access denied to delete other users' comments");
  }
  on.world.blocks.delete(commentId);
  on.world.deletedBlocks.set(commentId, {
    boardId: on.wikiId,
    blocks: [comment],
    ...deletion(on),
  });
  on.world.tellBoard(on.wikiId, {
    kind: 'blocksDeleted',
    boardId: on.wikiId,
    blockIds: [commentId],
  });
}

/** Also of a comment deleted on its own, whose page is there: as the server keeps them. */
export function versionsOf(on: OnPage, commentId: string): CommentVersion[] {
  memberRole(on.world, on.userId, on.wikiId, 'wiki');
  pageOn(on);
  const deleted = on.world.deletedBlocks.get(commentId)?.blocks[0];
  const comment = on.world.blocks.get(commentId) ?? deleted;
  if (comment?.type !== 'comment' || comment.parentId !== on.pageId) fail(404, 'no such comment');
  return earlierVersions(on.world.commentVersions.get(commentId) ?? []);
}

/**
 * The comments of `Wikis` in memory: what `MemoryWikis` is besides its pages,
 * each call waiting as the backend's do (`enter`) and asked as its user (`on`).
 */
export abstract class MemoryPageComments {
  protected abstract enter(method: keyof Wikis): Promise<void>;
  protected abstract on(wikiId: string, pageId: string): OnPage;

  async comments(wikiId: string, pageId: string): Promise<Comment[]> {
    await this.enter('comments');
    return commentsOf(this.on(wikiId, pageId));
  }

  async createComment(wikiId: string, pageId: string, body: DocNode): Promise<Comment> {
    await this.enter('createComment');
    return createComment(this.on(wikiId, pageId), body);
  }

  async patchComment(
    wikiId: string,
    pageId: string,
    commentId: string,
    write: BodyWrite,
  ): Promise<Comment> {
    await this.enter('patchComment');
    return patchComment(this.on(wikiId, pageId), commentId, write);
  }

  async deleteComment(wikiId: string, pageId: string, commentId: string): Promise<void> {
    await this.enter('deleteComment');
    deleteComment(this.on(wikiId, pageId), commentId);
  }

  async commentVersions(
    wikiId: string,
    pageId: string,
    commentId: string,
  ): Promise<CommentVersion[]> {
    await this.enter('commentVersions');
    return versionsOf(this.on(wikiId, pageId), commentId);
  }
}
