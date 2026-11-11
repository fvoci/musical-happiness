/**
 * What the share dialog works out: the roles in their order, the role a
 * membership stands for, and the public link of a board.
 */
import type { BoardMember } from '$lib/logic/wire.ts';
import type { BoardRole } from '$lib/state/index.ts';

/** The roles, highest first. */
export const ROLE_ORDER: readonly BoardRole[] = ['admin', 'editor', 'commenter', 'viewer'];

/**
 * The highest role a membership has. `none`: the server keeps memberships
 * without a role (they may not even view the board).
 */
export function roleOfMember(member: BoardMember): BoardRole | 'none' {
  if (member.schemeAdmin) return 'admin';
  if (member.schemeEditor) return 'editor';
  if (member.schemeCommenter) return 'commenter';
  return member.schemeViewer ? 'viewer' : 'none';
}

/**
 * The public link: the board's own address with its read token
 * (`readToken`, as the API names it). `boardAddress` is a whole address.
 */
export function publicLinkOf(boardAddress: string, token: string): string {
  const url = new URL(boardAddress);
  url.searchParams.set('readToken', token);
  return url.href;
}
