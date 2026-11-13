/**
 * Who is in a workspace, and the links that let others in (part of `backend.ts`).
 * The screens have their own word for a workspace; the names here
 * are the server's.
 */

import type { Teams } from './team-types.ts';

/** What somebody is in a workspace. A guest is on the boards they were invited to, and nothing else of it. */
export type WorkspaceRole = 'admin' | 'member' | 'guest';

/** The role a guest is invited to a board in. */
export type GuestBoardRole = 'viewer' | 'commenter' | 'editor';

/** Somebody who is in a workspace. */
export interface WorkspaceMember {
  userId: string;
  /** The name they sign in with. */
  username: string;
  /** The name they are shown by; empty for none. */
  displayName: string;
  role: WorkspaceRole;
}

/** A link that lets somebody into a workspace. */
export interface Invitation {
  id: string;
  /** Whom it makes of who accepts it: a member, or a guest of one board. */
  role: 'member' | 'guest';
  /** For a guest: the board. */
  boardId?: string;
  createdBy: string;
  /** When it was made and when it ends, in milliseconds since the epoch. */
  createAt: number;
  expiresAt: number;
  /** How often it was used. */
  uses: number;
  /** The token of the link: known when the link is made, and never again. */
  token?: string;
}

/**
 * Managing who is in a workspace, where a backend has that: the server has,
 * the one in memory has not. Every request is refused (403) to who is no
 * administrator of the workspace, but the links for guests, which a board's
 * admins make and see.
 */
export interface People {
  /** Present where the backend supports workspace groups. */
  readonly teams?: Teams;
  members(workspaceId: string): Promise<WorkspaceMember[]>;
  /** A change that is not made so (a guest to a member) is 400; the last administrator stays one (409, `last_admin`). */
  setRole(workspaceId: string, userId: string, role: WorkspaceRole): Promise<WorkspaceMember>;
  removeMember(workspaceId: string, userId: string): Promise<void>;
  /** The links that are not ended, used up or revoked: all of them for an administrator, one's own otherwise. Newest first. */
  invitations(workspaceId: string): Promise<Invitation[]>;
  /** A link for members, used as often as it is for seven days. It comes with its token. */
  inviteMembers(workspaceId: string): Promise<Invitation>;
  /** A link for one guest of a board, for seven days. It comes with its token. */
  inviteGuest(workspaceId: string, boardId: string, role: GuestBoardRole): Promise<Invitation>;
  revokeInvitation(workspaceId: string, invitationId: string): Promise<void>;
  /** Whether the admins of the workspace's boards may invite guests to them. */
  guestInvitations(workspaceId: string): Promise<boolean>;
  setGuestInvitations(workspaceId: string, enabled: boolean): Promise<boolean>;
}
