/** The memory directory distinguishes current members from the authors of readable work. */
import { emptyName, nameContains } from '../../logic/person-names.ts';
import { isGone } from '../../logic/users.ts';
import type { User } from '../backend.ts';

import { isOpenWiki } from './container-access.ts';
import { roleOn } from './comments.ts';
import { fail } from './refuse.ts';
import { workersOf } from './spaces.ts';
import { copy, scramble } from './shapes.ts';
import type { World } from './world.ts';

export function seedUsers(world: World, users: readonly User[]): void {
  for (const user of users) {
    world.users.set(user.id, user);
    world.workspaceMembers.add(user.id);
  }
}

/**
 * Whether a search for `wanted`, in small letters, finds the user, as the
 * server finds them: in the login name or the name (`nameContains`).
 */
export function userFound(user: User, wanted: string): boolean {
  return (
    user.username.toLowerCase().includes(wanted) ||
    user.displayName.toLowerCase().includes(wanted) ||
    nameContains(user.name ?? emptyName(), wanted)
  );
}

function listed(user: User): User {
  const { isAdmin: _isAdmin, ...rest } = copy(user);
  return rest;
}

/**
 * Deletes the account of a user, as the server erases one: it is in the
 * workspace and on its boards no more, nothing of the person is left (nor
 * when the account came or went: its times are zero), and what they wrote
 * stays under the identifier.
 */
export function eraseUser(world: World, userId: string): void {
  const user = world.users.get(userId);
  if (!user) return;
  world.users.set(userId, {
    ...user,
    username: '',
    displayName: '',
    name: emptyName(),
    email: '',
    isAdmin: false,
    createAt: 0,
    updateAt: 0,
    deleteAt: 0,
  });
  world.workspaceMembers.delete(userId);
  world.settings.delete(userId);
  for (const [boardId, members] of world.members) {
    world.members.set(
      boardId,
      members.filter((member) => member.userId !== userId),
    );
  }
}

/** The people who work in a workspace (`workersOf`), but who are gone. */
export function usersAmong(world: World, workers: ReadonlySet<string>): User[] {
  return scramble([...world.users.values()])
    .filter((user) => workers.has(user.id) && !isGone(user) && user.deleteAt === 0)
    .map(listed);
}

export function usersNamed(
  world: World,
  readerId: string,
  workspaceId: string,
  userIds: readonly string[],
): User[] {
  const workers = workersOf(world, workspaceId);
  if (!workers.has(readerId)) fail(404, 'no such workspace');
  const visible = new Set(workers);
  for (const board of world.containers.values()) {
    if (board.workspaceId !== workspaceId) continue;
    const role = roleOn(world.members.get(board.id) ?? [], readerId);
    if (!role && !(board.isTemplate && board.type === 'O') && !isOpenWiki(board)) continue;
    visible.add(board.createdBy);
    for (const block of world.blocksOf(board.id)) {
      visible.add(block.createdBy);
      visible.add(block.modifiedBy);
      if (block.type === 'comment') {
        for (const version of world.commentVersions.get(block.id) ?? [])
          visible.add(version.writtenBy);
      }
    }
    for (const deleted of world.deletedBlocks.values()) {
      if (deleted.containerId !== board.id) continue;
      for (const block of deleted.blocks) {
        if (block.type !== 'comment') continue;
        for (const version of world.commentVersions.get(block.id) ?? [])
          visible.add(version.writtenBy);
      }
    }
  }
  return (
    scramble([...world.users.values()])
      // As the server: a closed account is named by nobody, though its work stays; one
      // that was deleted is answered as what is left of it, to whoever asks.
      .filter(
        (user) =>
          userIds.includes(user.id) &&
          (isGone(user) || (visible.has(user.id) && user.deleteAt === 0)),
      )
      .map(listed)
  );
}
