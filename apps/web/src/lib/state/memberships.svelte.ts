/**
 * Of the open containers no category of the reader's files, the ones the
 * reader is known to be a member of: directly, or by a group (a membership
 * with an `inheritedRole`), as read where each was opened. The server files a
 * container in the reader's categories when they join it or are added to it,
 * not when a group of theirs is given a role on it: such a one is the
 * reader's all the same, listed with their own (`sidebarOf`), not among those
 * that wait to be joined (`openToJoinOf`). One for each `Workspace`.
 */
import { SvelteSet } from 'svelte/reactivity';

import type { ContainerMember } from '../logic/container-members.ts';

import { roleOf } from './board-parts.ts';
import type { SidebarReader } from './order.ts';
import type { Workspace } from './workspace.svelte.ts';

const ofWorkspace = new WeakMap<Workspace, SvelteSet<string>>();

function membershipsOf(workspace: Workspace): SvelteSet<string> {
  let known = ofWorkspace.get(workspace);
  if (!known) {
    known = new SvelteSet();
    ofWorkspace.set(workspace, known);
  }
  return known;
}

/** Notes, from the members of `containerId` as read, whether the reader is one of them. */
export function noteMembership(
  workspace: Workspace,
  containerId: string,
  members: readonly ContainerMember[],
): void {
  const meId = workspace.me?.id;
  const member = members.some((one) => one.userId === meId && roleOf(one) !== '');
  const known = membershipsOf(workspace);
  if (member) known.add(containerId);
  else known.delete(containerId);
}

/** The reader of `workspace`'s sidebar: who they are, whether a guest, and what they are known to be a member of. */
export function readerOf(workspace: Workspace): SidebarReader {
  return {
    id: workspace.me?.id,
    guest: workspace.info?.role === 'guest',
    memberOf: membershipsOf(workspace),
  };
}
