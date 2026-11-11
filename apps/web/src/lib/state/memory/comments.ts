/**
 * The memory backend's comments as the server has them: who may change or
 * delete one (`patchBlock` and `deleteBlock` in `contracts/openapi.json`),
 * and the texts kept of one that was changed (`getCommentVersions`).
 */
import type { DocNode } from '../../editor/schema.ts';
import type { BoardMember } from '../../logic/wire.ts';
import type { CommentVersion } from '../backend.ts';

import type { StoredBlock } from './world.ts';

export type Role = 'admin' | 'editor' | 'commenter' | 'viewer' | '';

/** The highest role of a user on a board; none for who is no member. */
export function roleOn(members: readonly BoardMember[], userId: string): Role {
  const member = members.find((each) => each.userId === userId);
  if (!member) return '';
  if (member.schemeAdmin) return 'admin';
  if (member.schemeEditor) return 'editor';
  if (member.schemeCommenter) return 'commenter';
  return member.schemeViewer ? 'viewer' : '';
}

/** Its author changes a comment with the commenter's role; anybody else's is for the admins. */
export function mayChangeComment(role: Role, isAuthor: boolean): boolean {
  if (role === 'admin') return true;
  return isAuthor && (role === 'editor' || role === 'commenter');
}

/** Its author deletes a comment, and brings it back, with the commenter's role; anybody else's is for the admins. */
export function mayDeleteComment(role: Role, isAuthor: boolean): boolean {
  if (role === 'admin') return true;
  return isAuthor && (role === 'editor' || role === 'commenter');
}

/**
 * When the newest comment directly on the card was posted; 0 for none. A
 * deleted one is in the trash and does not count.
 */
export function lastCommentAt(blocks: readonly StoredBlock[]): number {
  return Math.max(0, ...blocks.filter((block) => block.type === 'comment').map((c) => c.createAt));
}

/** How many texts before the one it has now a comment keeps: the server's default. */
export const COMMENT_VERSIONS_KEPT = 50;

/** A comment's text as it was: what a change writes down. */
export interface CommentText {
  body: DocNode | null;
  by: string;
  at: number;
}

/**
 * Writes down a change of a comment's text. `versions` holds every text
 * written down, the one it has now last; the first change also writes the
 * text it was posted with (`posted`). The first and the newest are kept,
 * `COMMENT_VERSIONS_KEPT` before the one it has now.
 */
export function recordChange(
  versions: readonly CommentVersion[],
  posted: CommentText,
  now: CommentText,
): CommentVersion[] {
  const kept = versions.length > 0 ? [...versions] : [versionOf(1, posted)];
  const newest = kept.at(-1)?.version ?? 1;
  kept.push(versionOf(newest + 1, now));
  const cut = newest + 1 - COMMENT_VERSIONS_KEPT;
  return kept.filter((version, index) => index === 0 || version.version > cut);
}

/** The texts of a comment before the one it has now, the first first. */
export function earlierVersions(versions: readonly CommentVersion[]): CommentVersion[] {
  return structuredClone(versions.slice(0, -1));
}

function versionOf(version: number, text: CommentText): CommentVersion {
  return { version, body: structuredClone(text.body), writtenBy: text.by, writtenAt: text.at };
}
