/** The reader's own account in the world in memory, changed as the server changes it. */
import { readDisplayName } from '../../logic/users.ts';
import type { User } from '../backend.ts';

import { fail } from './refuse.ts';
import type { World } from './world.ts';

/**
 * Sets a user's display name as the server does (`patchMe`): without the
 * white space around it, empty or `null` for none, refused at `/displayName`
 * with the server's reason. Answers the user as stored.
 */
export function changeDisplayName(world: World, userId: string, name: string | null): User {
  const user = world.users.get(userId) ?? fail(401, 'no session');
  const read = readDisplayName(name ?? '');
  if ('refused' in read) {
    fail(400, 'the display name is refused', { field: 'displayName', reason: read.refused });
  }
  user.displayName = read.name;
  user.updateAt = world.now();
  return user;
}
