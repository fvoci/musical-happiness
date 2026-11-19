/** The reader's own account: what the workspace changes of it beside the settings. */
import type { NameChange } from '../logic/person-names.ts';

import type { User } from './backend.ts';
import type { Workspace } from './workspace.svelte.ts';

/**
 * Changes what the patch names of the reader's name. Every list of people
 * the workspace holds then has the new name. A refusal is thrown, for the
 * field to say what is wrong (`errors` at the member, `/firstName`).
 */
export async function changeName(workspace: Workspace, patch: NameChange): Promise<void> {
  takeUser(workspace, await workspace.backend.patchMe(patch));
}

/** Puts the user as they are now wherever the workspace holds them. */
export function takeUser(workspace: Workspace, changed: User): void {
  const replace = (users: User[]): User[] =>
    users.map((user) => (user.id === changed.id ? changed : user));
  // Kept from the reader's own reading where another reading leaves it out.
  const me = workspace.me;
  if (me?.id === changed.id) workspace.me = { ...changed, isAdmin: changed.isAdmin ?? me.isAdmin };
  workspace.users = workspace.user(changed.id)
    ? replace(workspace.users)
    : [...workspace.users, changed];
  workspace.workspaceUsers = replace(workspace.workspaceUsers);
}
