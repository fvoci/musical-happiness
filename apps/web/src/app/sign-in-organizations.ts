/**
 * The reader's organizations that sign in through a provider: what the
 * connected sign-ins of the settings offer to connect, each once, whichever
 * of its workspaces the reader is in. One whose provider is off, or that has
 * none, is left out.
 */
import type { LoginOptions, SignInOrganization, WorkspaceInfo } from '$lib/state/index.ts';

export async function signInOrganizations(
  workspaces: readonly WorkspaceInfo[],
  loginOptions: (orgId: string) => Promise<LoginOptions>,
): Promise<SignInOrganization[]> {
  // An organization once, by the workspaces of it the reader is in: those are the
  // names they know it by. Its own name stands in where none of them has one.
  const titles = new Map<string, string[]>();
  for (const workspace of workspaces) {
    if (workspace.orgId === undefined) continue;
    const known = titles.get(workspace.orgId) ?? [];
    titles.set(workspace.orgId, workspace.title ? [...known, workspace.title] : known);
  }
  const found = await Promise.all(
    [...titles].map(async ([orgId, known]) => {
      // One that cannot be read is not offered; the others still are.
      const options = await loginOptions(orgId).catch(() => undefined);
      const organization = options?.organization;
      if (!organization?.oidc) return undefined;
      return { orgId, name: known.join(', ') || organization.name || orgId };
    }),
  );
  return found.filter((one) => one !== undefined);
}
