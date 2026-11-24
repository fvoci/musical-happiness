/** Unit notification subscriptions in the in-memory backend. */
import { reads } from './container-access.ts';
import { fail } from './refuse.ts';
import { checkWorkspace } from './spaces.ts';
import type { World } from './world.ts';

export function setUnitWatch(
  world: World,
  userId: string,
  unitId: string,
  watching: boolean,
): void {
  let byUnit = world.unitWatchers.get(userId);
  if (!byUnit) {
    byUnit = new Map();
    world.unitWatchers.set(userId, byUnit);
  }
  byUnit.set(unitId, watching);
}

export function memoryWatchingUnits(
  world: World,
  sessionUserId: string,
  workspaceId: string,
  userId: string,
): string[] {
  checkWorkspace(world, sessionUserId, workspaceId);
  if (userId !== sessionUserId) fail(404, 'no such user');
  const explicit = world.unitWatchers.get(userId);
  const ids: string[] = [];
  for (const block of world.blocks.values()) {
    if (block.type !== 'unit') continue;
    const container = world.containers.get(block.containerId);
    if (container?.workspaceId !== workspaceId || !reads(world, userId, container.id)) {
      continue;
    }
    const override = explicit?.get(block.id);
    if (override === false) continue;
    const auto =
      workspaceId === world.workspace.id &&
      (block.createdBy === userId ||
        world
          .childrenOf(block.id)
          .some((child) => child.type === 'comment' && child.createdBy === userId));
    if (override === true || auto) ids.push(block.id);
  }
  return ids;
}

export function memoryWatchUnit(
  world: World,
  sessionUserId: string,
  workspaceId: string,
  unitId: string,
  userId: string,
): void {
  checkWorkspace(world, sessionUserId, workspaceId);
  if (userId !== sessionUserId) {
    fail(403, 'a user subscribes for themselves only', { field: 'userId' });
  }
  const block = world.blocks.get(unitId);
  if (
    !block ||
    world.containers.get(block.containerId)?.workspaceId !== workspaceId ||
    !reads(world, sessionUserId, block.containerId) ||
    block.type !== 'unit'
  ) {
    fail(404, 'no such block');
  }
  setUnitWatch(world, userId, unitId, true);
}

export function memoryUnwatchUnit(
  world: World,
  sessionUserId: string,
  workspaceId: string,
  unitId: string,
  userId: string,
): void {
  checkWorkspace(world, sessionUserId, workspaceId);
  if (userId !== sessionUserId) fail(404, 'no such subscription');
  setUnitWatch(world, userId, unitId, false);
}
