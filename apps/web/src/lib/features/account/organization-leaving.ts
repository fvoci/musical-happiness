/**
 * Leaving an organization from the settings: which organizations the reader
 * can leave, by the names they know them by, and what the dialog says of a
 * refusal. The words come from the refusal's status and reason, never from
 * the server's detail.
 */
import { m } from '$lib/i18n/paraglide/messages.js';
import { isBackendError, isOwnSpace, type WorkspaceInfo } from '$lib/state/index.ts';

/** An organization the reader is a member of, by the names of its workspaces they are in. */
export interface LeavableOrganization {
  orgId: string;
  name: string;
}

/**
 * The organizations that people work in together that the reader is a member
 * of, each once, in the order of their workspaces. One where the reader is a
 * guest of every workspace is none they are a member of: a guest leaves a
 * board, not the organization. Their own space is left by nobody.
 */
export function leavableOrganizations(
  workspaces: readonly WorkspaceInfo[],
): LeavableOrganization[] {
  const titles = new Map<string, string[]>();
  for (const workspace of workspaces) {
    if (workspace.orgId === undefined || isOwnSpace(workspace) || workspace.role === 'guest') {
      continue;
    }
    const known = titles.get(workspace.orgId) ?? [];
    titles.set(workspace.orgId, workspace.title ? [...known, workspace.title] : known);
  }
  return [...titles].map(([orgId, known]) => ({
    orgId,
    name: known.join(', ') || m.account_delete_organization_unnamed(),
  }));
}

export interface LeaveProblem {
  text: string;
  /** Asking again changes nothing until the reader does what the text says. */
  settled: boolean;
}

export function leaveProblemOf(error: unknown): LeaveProblem {
  if (!isBackendError(error)) return { text: m.account_failed(), settled: false };
  if (error.status === 404) return { text: m.org_leave_not_member(), settled: true };
  switch (error.reason) {
    case 'last_instance_admin':
      return { text: m.org_leave_last_instance_admin(), settled: true };
    case 'sole_org_admin':
      return { text: m.org_leave_sole_admin(), settled: true };
    case 'alone_in_organization':
      return { text: m.org_leave_alone(), settled: true };
    default:
      return { text: m.account_failed(), settled: false };
  }
}
