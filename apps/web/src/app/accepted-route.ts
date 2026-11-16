/**
 * Where an accepted invitation leads: into its workspace, or for a guest to
 * the board or the wiki they were invited to, each at its own address (the
 * answer says which; one that does not is a board's).
 */
import type { AcceptedInvitation } from '$lib/state/index.ts';

import type { Route } from './router.svelte.ts';

export function acceptedRoute(accepted: AcceptedInvitation): Partial<Route> {
  const { workspaceId, containerId, containerKind } = accepted;
  if (!containerId) return { workspaceId };
  return containerKind === 'wiki'
    ? { workspaceId, wikiId: containerId }
    : { workspaceId, boardId: containerId };
}
