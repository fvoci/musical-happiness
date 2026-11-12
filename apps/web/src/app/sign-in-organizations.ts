/**
 * The reader's organizations that sign in through a provider: what the
 * connected sign-ins of the settings offer to connect, each once, whichever
 * of its workspaces the reader is in. One whose provider is off, or that has
 * none, is left out.
 */
import type { LoginOrganization, SignInOrganization, WorkspaceInfo } from '$lib/state/index.ts';

export async function signInOrganizations(
  workspaces: readonly WorkspaceInfo[],
  loginOrganization: (orgId: string) => Promise<LoginOrganization | undefined>,
): Promise<SignInOrganization[]> {
  // An organization once, under the name of its first workspace if it has none of its own.
  const titles = new Map<string, string>();
  for (const workspace of workspaces) {
    if (workspace.orgId !== undefined && !titles.has(workspace.orgId)) {
      titles.set(workspace.orgId, workspace.title);
    }
  }
  const found = await Promise.all(
    [...titles].map(async ([orgId, title]) => {
      // One that cannot be read is not offered; the others still are.
      const organization = await loginOrganization(orgId).catch(() => undefined);
      return organization?.oidc ? { orgId, name: organization.name || title } : undefined;
    }),
  );
  return found.filter((one) => one !== undefined);
}
