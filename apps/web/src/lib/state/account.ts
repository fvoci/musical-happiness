/** The reader's own account: what the workspace changes of it beside the settings. */
import type { NameChange } from '../logic/person-names.ts';

import { BackendError, isOwnSpace, type User } from './backend.ts';
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
 * Gives `password`, which the account was just given, in the session where
 * the reader's own space asks for it (`Backend.confirmPassword`): a session
 * made through a provider, as the server lists the workspaces to it. The
 * reader is not asked for what they typed a moment ago. Whether the own
 * space is open to the session afterwards; a refusal (too many tries, a
 * password changed again meanwhile) or a failed reading leaves it as it is.
 */
async function showPassword(workspace: Workspace, password: string): Promise<boolean> {
  try {
    let listed = await workspace.backend.workspaces();
    if (listed.some((one) => one.passwordRequired)) {
      await workspace.backend.confirmPassword(password);
      listed = await workspace.backend.workspaces();
    }
    workspace.workspaces = listed;
    return !listed.some((one) => one.passwordRequired);
  } catch {
    return false;
  }
}

/**
 * The reader gave the account its first password: it has one now, and its own
 * space takes only a session that has shown it. This one shows the password
 * it was just given, and the space, which the server stopped telling it of,
 * is entered again where it is shown. Where that is refused the page goes on
 * as the server answers it from now on (`Workspace.report`): the own space
 * asks for the password.
 */
export async function passwordSet(workspace: Workspace, password: string): Promise<void> {
  const me = workspace.me;
  if (me) workspace.me = { ...me, hasPassword: true };
  if (await showPassword(workspace, password)) {
    const shown = workspace.info ?? workspace.closed;
    if (workspace.asked !== undefined && shown && isOwnSpace(shown)) {
      await workspace.enter(workspace.asked, true);
    }
    return;
  }
  workspace.report(
    'load',
    new BackendError(401, { reason: 'password_sign_in_required', detail: 'a password was set' }),
  );
}

/**
 * The reader changed the account's password in this session: where its own
 * space asked the session for the password, the session shows the new one,
 * and the space, if it is the one shown, opens.
 */
export async function passwordChanged(workspace: Workspace, password: string): Promise<void> {
  if (!(await showPassword(workspace, password))) return;
  if (workspace.entered === 'password' && workspace.asked !== undefined) {
    await workspace.enter(workspace.asked, true);
  }
}

/**
 * The reader gives the account's password in the session they have
 * (`Backend.confirmPassword`), and their own space opens to it where it is:
 * no other session is made, and nothing else of the page starts again. The
 * workspaces are read as the server lists them to the session now, and the
 * one shown, if it is still the space that asked, is entered. A refusal (a
 * wrong password, too many tries) rejects, for the form to say.
 */
export async function confirmPassword(workspace: Workspace, password: string): Promise<void> {
  await workspace.backend.confirmPassword(password);
  workspace.workspaces = await workspace.backend.workspaces();
  // Not a workspace the reader went to while this was under way.
  if (workspace.entered === 'password' && workspace.asked !== undefined) {
    await workspace.enter(workspace.asked, true);
  }
}

/**
 * Reads the account's address anew, and the one it waits to change to: after
 * the reader asked for a change or took one back, and where the settings show
 * them, since the link is mostly opened in another browser. A failed reading
 * leaves them as they were.
 */
export async function rereadEmail(workspace: Workspace): Promise<void> {
  const read = await workspace.backend.me().catch(() => undefined);
  const me = workspace.me;
  if (read && me?.id === read.id) {
    workspace.me = { ...me, email: read.email, pendingEmail: read.pendingEmail };
  }
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
      email: changed.email || me.email,
      pendingEmail: changed.pendingEmail ?? me.pendingEmail,
    };
  }
  workspace.users = workspace.user(changed.id)
    ? replace(workspace.users)
    : [...workspace.users, changed];
  workspace.workspaceUsers = replace(workspace.workspaceUsers);
}
