/** Content belongs to members: open boards ask others to join; private ones are missing. */
import type { BoardKind } from '../../logic/wire.ts';

import { roleOn, type Role } from './comments.ts';
import { fail } from './refuse.ts';
import type { World } from './world.ts';

/**
 * The reader's role on a board of the `kind` (the server answers a wiki to an
 * operation of cards as a board that is not there); `any` for one of either.
 */
export function memberRole(
  world: World,
  userId: string,
  boardId: string,
  kind: BoardKind | 'any' = 'board',
): Exclude<Role, ''> {
  const board = world.boards.get(boardId);
  if (!board || (kind !== 'any' && (board.kind ?? 'board') !== kind)) fail(404, 'no such board');
  const role = roleOn(world.members.get(boardId) ?? [], userId);
  if (role !== '') return role;
  if (board.type === 'O') fail(403, 'access denied to board');
  fail(404, 'no such board');
}

/** A local blob URL grants direct access, so do not make or return it for a nonmember. */
export function memberFileUrl(
  world: World,
  userId: string,
  boardId: string,
  fileId: string,
): string | undefined {
  if (!world.boards.has(boardId) || roleOn(world.members.get(boardId) ?? [], userId) === '')
    return undefined;
  const file = world.files.get(fileId);
  if (file?.boardId !== boardId || !file.blob) return undefined;
  file.url ??= URL.createObjectURL(file.blob);
  return file.url;
}
