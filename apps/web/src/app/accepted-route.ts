/**
 * Where an accepted invitation leads: into its workspace, or for a guest to
 * the board or the wiki they were invited to, each at its own address. The
 * answer of the invitation does not say which of the two it is: the wikis of
 * the workspace are read, as the one who just came in, under the workspace
 * the answer names (none is entered yet). Not among them, or not read, it is
 * asked for at a board's address, which tells what there is to tell.
 */
import type { AcceptedInvitation, Backend } from '$lib/state/index.ts';

import type { Route } from './router.svelte.ts';

export async function acceptedRoute(
  backend: Pick<Backend, 'wiki'>,
  accepted: AcceptedInvitation,
): Promise<Partial<Route>> {
  const { workspaceId, boardId } = accepted;
  if (!boardId) return { workspaceId };
  const wikis = await backend.wiki?.wikis(workspaceId).catch(() => []);
  return wikis?.some((wiki) => wiki.id === boardId)
    ? { workspaceId, wikiId: boardId }
    : { workspaceId, boardId };
}
