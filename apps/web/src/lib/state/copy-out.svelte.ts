/**
 * The organizations known to keep their boards, wikis and courses in: those
 * whose administrators switched copying out off (`allowCopyOut`). Only an
 * administrator reads that switch (`OrgAdmin`); anybody else learns it from
 * the refusal of a copy (`copy_out_disabled`). What was learnt holds for the
 * life of the page, until the switch is read again.
 */
import { SvelteSet } from 'svelte/reactivity';

import type { WorkspaceInfo } from './workspace-types.ts';

const keptIn = new SvelteSet<string>();

/**
 * What a workspace's containers are copied out of: its organization, or the
 * workspace itself on a backend without organizations.
 */
export function copyOutPlace(workspace: Pick<WorkspaceInfo, 'id' | 'orgId'>): string {
  return workspace.orgId ?? workspace.id;
}

/** Whether the place is known to refuse copies out of it. */
export function keepsCopiesIn(place: string): boolean {
  return keptIn.has(place);
}

/** What was learnt of the place: whether it lets its containers be copied out. */
export function noteCopyOut(place: string, allowed: boolean): void {
  if (allowed) keptIn.delete(place);
  else keptIn.add(place);
}
