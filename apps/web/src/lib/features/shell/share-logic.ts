/**
 * What the share dialog works out: the roles in their order, and the role a
 * membership stands for. The public link's address is the router's.
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
