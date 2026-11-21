/**
 * Content belongs to members: open boards ask others to join; private ones are
 * missing. An open wiki is read by whoever works in the workspace, as a viewer
 * reads it, without joining it: only joined do they write in it. This world has
 * no guests: everybody in it works there.
 */
import type { ContainerKind } from '../../logic/wire.ts';

import { roleOn, type Role } from './comments.ts';
import { fail } from './refuse.ts';
import { workersOf } from './spaces.ts';
import type { World } from './world.ts';

/**
 * The reader's role on a board or a wiki of the `kind` (the server answers a wiki to an
 * operation of cards as a board that is not there); `any` for one of either.
 */
export function memberRole(
  world: World,
  userId: string,
  containerId: string,
  kind: ContainerKind | 'any' = 'board',
): Exclude<Role, ''> {
  const container = world.containers.get(containerId);
  if (!container || (kind !== 'any' && (container.kind ?? 'board') !== kind))
    fail(404, 'no such board');
  const role = roleOn(world.members.get(containerId) ?? [], userId);
  if (role !== '') return role;
  // Of a workspace the user is not in (somebody else's own space), nothing is there.
  const works = workersOf(world, container.workspaceId).has(userId);
  if (isOpenWiki(container) && works) return 'viewer';
  if (container.type === 'O' && works) fail(403, 'access denied to board');
  fail(404, 'no such board');
}

/**
 * Whether the user reads the board or wiki: has a role on it, or works in the
 * workspace of an open wiki.
 */
export function reads(world: World, userId: string, containerId: string): boolean {
  const container = world.containers.get(containerId);
  if (!container) return false;
  if (roleOn(world.members.get(containerId) ?? [], userId) !== '') return true;
  return isOpenWiki(container) && workersOf(world, container.workspaceId).has(userId);
}

/** An open wiki: read by whoever works in its workspace. */
export function isOpenWiki(container: { type: string; kind?: string }): boolean {
  return container.type === 'O' && container.kind === 'wiki';
}

/** A local blob URL grants direct access, so do not make or return it for a nonmember. */
export function memberFileUrl(
  world: World,
  userId: string,
  containerId: string,
  fileId: string,
): string | undefined {
  if (!reads(world, userId, containerId)) return undefined;
  const file = world.files.get(fileId);
  if (file?.containerId !== containerId || !file.blob) return undefined;
  file.url ??= URL.createObjectURL(file.blob);
  return file.url;
}
