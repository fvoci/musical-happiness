/** A workspace of the reader's, as their list has it (part of `backend.ts`). */
import type { WorkspaceRole } from './people-types.ts';

/**
 * What the organization of a workspace is: `personal` for the reader's own
 * space ("My space"), which is theirs alone and has nobody to administer it;
 * `team` for every other.
 */
export type OrgKind = 'team' | 'personal';

export interface WorkspaceInfo {
  id: string;
  /** Empty if it was given none, as the reader's own space always is: a screen names that one. */
  title: string;
  /** The organization the workspace belongs to, where the backend has organizations (the server). */
  orgId?: string;
  /** What that organization is; left out by a backend that does not say, as for a `team`. */
  kind?: OrgKind;
  /**
   * The workspace is closed to this session: its organization takes no
   * password, and it opens for a session made through the provider of `orgId`.
   */
  ssoRequired?: boolean;
  /**
   * What the reader is in it; left out by a backend that has no roles, where
   * the reader does everything. A guest is on the boards they were invited
   * to, and makes none.
   */
  role?: WorkspaceRole;
}

/**
 * Whether the workspace is the reader's own space: nobody else is in it, so
 * there is nobody to invite, no team and no organization to administer;
 * what is in it is shown to others by a public link alone.
 */
export const isOwnSpace = (workspace: WorkspaceInfo | undefined): boolean =>
  workspace?.kind === 'personal';
