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
  const changed = await workspace.backend.patchMe(patch);
  const replace = (users: User[]): User[] =>
    users.map((user) => (user.id === changed.id ? changed : user));
  workspace.me = changed;
  workspace.users = workspace.user(changed.id)
    ? replace(workspace.users)
    : [...workspace.users, changed];
  workspace.workspaceUsers = replace(workspace.workspaceUsers);
}
