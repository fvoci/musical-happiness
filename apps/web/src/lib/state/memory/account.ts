/** The reader's own account in the world in memory, changed as the server changes it. */
import {
  CREDENTIALS,
  emptyName,
  MAX_CREDENTIALS,
  NAME_SHOWN,
  shownName,
  TITLES,
  type NameChange,
  type PersonName,
} from '../../logic/person-names.ts';
import { readDisplayName } from '../../logic/users.ts';
import type { User } from '../backend.ts';

import { fail } from './refuse.ts';
import type { World } from './world.ts';

/** The members that are parts of a name, read as a display name is. */
const PARTS = ['firstName', 'lastName', 'latinFirstName', 'latinLastName', 'customName'] as const;

/** A part of a name as the server reads it, or its refusal at the member. */
function part(member: string, value: unknown): string {
  if (value === null) return '';
  if (typeof value !== 'string') {
    fail(400, `${member} must be a string or null`, { field: member, reason: 'invalid' });
  }
  const read = readDisplayName(value);
  if ('refused' in read) fail(400, `${member} is refused`, { field: member, reason: read.refused });
  return read.name;
}

function credentials(value: unknown): PersonName['credentials'] {
  if (value === null) return [];
  if (!Array.isArray(value)) {
    fail(400, 'credentials must be a list', { field: 'credentials', reason: 'invalid' });
  }
  if (value.length > MAX_CREDENTIALS) {
    fail(400, 'at most three credentials', { field: 'credentials', reason: 'too_long' });
  }
  const read: PersonName['credentials'] = [];
  for (const [index, code] of value.entries()) {
    const known = CREDENTIALS.find((credential) => credential === code);
    if (!known || read.includes(known)) {
      fail(400, 'a credential is a code of its list, once', {
        field: String(index),
        within: 'credentials',
        reason: 'invalid',
      });
    }
    read.push(known);
  }
  return read;
}

/**
 * Changes what the patch names of a user's name as the server does
 * (`patchMe`): a part without the white space around it, empty or `null` for
 * none; a code off its list refused at its member, with the server's reason.
 * `displayName` is made anew from the name. Answers the user as stored. Unlike
 * the server, which names every refused member, it names the first alone.
 */
export function changeName(world: World, userId: string, patch: NameChange): User {
  const user = world.users.get(userId) ?? fail(401, 'no session');
  const name: PersonName = { ...emptyName(), ...user.name };
  if (!user.name) name.customName = user.displayName;
  for (const [member, value] of Object.entries(patch) as [string, unknown][]) {
    const asPart = PARTS.find((known) => known === member);
    // Left out of the JSON the server is sent, and so left as it is; so is a choice of `null`.
    if (value === undefined || (member === 'nameShown' && value === null)) continue;
    if (asPart) name[asPart] = part(member, value);
    else if (member === 'nameShown') {
      name.nameShown =
        NAME_SHOWN.find((shown) => shown === value) ??
        fail(400, 'nameShown is a code of its list', { field: member, reason: 'invalid' });
    } else if (member === 'title') {
      name.title =
        value === null
          ? null
          : (TITLES.find((title) => title === value) ??
            fail(400, 'title is a code of its list', { field: member, reason: 'invalid' }));
    } else if (member === 'credentials') name.credentials = credentials(value);
    else fail(400, `there is no member ${member}`, { field: member, reason: 'unknown' });
  }
  user.name = name;
  user.displayName = shownName(name);
  user.updateAt = world.now();
  // As the server tells it: whoever is told reads the person again, by the rule of who sees whom.
  if (Object.values(patch as Record<string, unknown>).some((value) => value !== undefined)) {
    world.tellUsers([...world.users.keys()], { kind: 'user', userId });
  }
  return user;
}
