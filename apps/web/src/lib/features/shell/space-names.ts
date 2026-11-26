/**
 * How a workspace is named where the reader tells theirs apart: the own space
 * as "My space"; one that is its organization's only workspace of the reader's
 * by the organization's name alone; one of several by "organization · workspace",
 * and grouped under the organization in the switcher.
 */
import { nameOfWorkspace } from '$lib/components/workspace-name.ts';
import { m } from '$lib/i18n/paraglide/messages.js';
import { isOwnSpace, type WorkspaceInfo } from '$lib/state/index.ts';

/** The name of an organization by its identifier; empty where it is not known. */
export type OrgName = (orgId: string | undefined) => string;

/** A workspace of the switcher: its line's label, and the organization it is grouped under. */
export interface SpaceLine {
  workspace: WorkspaceInfo;
  label: string;
  /** The organization's name, on the first line of its group only, for one of several workspaces. */
  heading?: string;
  /** The whole name, "organization · workspace", for one under a heading. */
  fullName: string;
}

const titleOf = (workspace: WorkspaceInfo | undefined): string =>
  nameOfWorkspace(workspace, m.org_untitled_workspace);

/** How many of `workspaces` each organization has. */
function countsOf(workspaces: readonly WorkspaceInfo[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const one of workspaces) {
    if (one.orgId && !isOwnSpace(one)) counts.set(one.orgId, (counts.get(one.orgId) ?? 0) + 1);
  }
  return counts;
}

/** The name `workspace` goes by among `workspaces`, as the top bar and the switcher's button show it. */
export function spaceName(
  workspace: WorkspaceInfo | undefined,
  workspaces: readonly WorkspaceInfo[],
  orgName: OrgName,
): string {
  if (!workspace || isOwnSpace(workspace)) return titleOf(workspace);
  const org = orgName(workspace.orgId);
  if (!org || !workspace.orgId) return titleOf(workspace);
  if ((countsOf(workspaces).get(workspace.orgId) ?? 0) < 2) return org;
  return m.space_of_org({ org, space: titleOf(workspace) });
}

/**
 * The switcher's lines: the own space first, then each organization's
 * workspaces together in the order the first of them is listed; an
 * organization of several is headed by its name.
 */
export function spaceLines(workspaces: readonly WorkspaceInfo[], orgName: OrgName): SpaceLine[] {
  const counts = countsOf(workspaces);
  const own = workspaces.filter(isOwnSpace);
  const shared = workspaces.filter((one) => !isOwnSpace(one));
  const orgs = [...new Set(shared.map((one) => one.orgId))];
  const lines: SpaceLine[] = own.map((workspace) => ({
    workspace,
    label: titleOf(workspace),
    fullName: titleOf(workspace),
  }));
  for (const orgId of orgs) {
    const inOrg = shared.filter((one) => one.orgId === orgId);
    const org = orgName(orgId);
    const several = org !== '' && (counts.get(orgId ?? '') ?? 0) >= 2;
    for (const [index, workspace] of inOrg.entries()) {
      const fullName = spaceName(workspace, workspaces, orgName);
      lines.push({
        workspace,
        label: several ? titleOf(workspace) : fullName,
        fullName,
        ...(several && index === 0 ? { heading: org } : {}),
      });
    }
  }
  return lines;
}
