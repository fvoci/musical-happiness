/** The reader's own account: what the workspace changes of it beside the settings. */
import type { NameChange } from '../logic/person-names.ts';

import { BackendError, type User } from './backend.ts';
import type { Workspace } from './workspace.svelte.ts';

/**
 * Changes what the patch names of the reader's name. Every list of people
 * the workspace holds then has the new name. A refusal is thrown, for the
 * field to say what is wrong (`errors` at the member, `/firstName`).
 */
export async function changeName(workspace: Workspace, patch: NameChange): Promise<void> {
  takeUser(workspace, await workspace.backend.patchMe(patch));
}

/**
 * The reader gave the account its first password: it has one now, and its own
 * space takes only a session made with it, which this one was not. The page
 * goes on as the server answers it from now on (`Workspace.report`): the own
 * space, shown or not, is closed to this session until the reader signs in
 * with the password.
 */
export function passwordSet(workspace: Workspace): void {
  const me = workspace.me;
  if (me) workspace.me = { ...me, hasPassword: true };
  workspace.report(
    'load',
    new BackendError(401, { reason: 'password_sign_in_required', detail: 'a password was set' }),
  );
}

/** Puts the user as they are now wherever the workspace holds them. */
export function takeUser(workspace: Workspace, changed: User): void {
  const replace = (users: User[]): User[] =>
    users.map((user) => (user.id === changed.id ? changed : user));
  // Kept from the reader's own reading where another reading leaves them out.
  const me = workspace.me;
  if (me?.id === changed.id) {
    workspace.me = {
      ...changed,
      isAdmin: changed.isAdmin ?? me.isAdmin,
      hasPassword: changed.hasPassword ?? me.hasPassword,
    };
  }
  workspace.users = workspace.user(changed.id)
    ? replace(workspace.users)
    : [...workspace.users, changed];
  workspace.workspaceUsers = replace(workspace.workspaceUsers);
}
