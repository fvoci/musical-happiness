/** The memory directory distinguishes current members from the authors of readable work. */
import type { User } from '../backend.ts';

import { isOpenWiki } from './container-access.ts';
import { roleOn } from './comments.ts';
import { fail } from './refuse.ts';
import { copy, scramble } from './shapes.ts';
import type { World } from './world.ts';

export function seedUsers(world: World, users: readonly User[]): void {
  for (const user of users) {
    world.users.set(user.id, user);
    world.workspaceMembers.add(user.id);
  }
}

function listed(user: User): User {
  const { isAdmin: _isAdmin, ...rest } = copy(user);
  return rest;
}

export function usersAmong(world: World): User[] {
  return scramble([...world.users.values()])
    .filter((user) => world.workspaceMembers.has(user.id) && user.deleteAt === 0)
    .map(listed);
}

export function usersNamed(world: World, readerId: string, userIds: readonly string[]): User[] {
  if (!world.workspaceMembers.has(readerId)) fail(404, 'no such workspace');
  const visible = new Set(world.workspaceMembers);
  for (const board of world.containers.values()) {
    if (board.workspaceId !== world.workspace.id) continue;
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
      // As the server: a closed account is named by nobody, though its work stays.
      .filter((user) => userIds.includes(user.id) && visible.has(user.id) && user.deleteAt === 0)
      .map(listed)
  );
}
