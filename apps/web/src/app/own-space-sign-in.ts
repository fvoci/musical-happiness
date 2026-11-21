/**
 * The way into the reader's own space when it takes only a session made with
 * the account's password: signing in with it, under the account's user name,
 * makes such a session in place of this one, and the page starts afresh at
 * its address, where the own space now opens. A refusal (a wrong password,
 * too many tries) rejects, for the form to say.
 */
import type { Account, Workspace } from '$lib/state/index.ts';

export async function signInWithPassword(
  account: Account | undefined,
  workspace: Workspace | undefined,
  password: string,
): Promise<void> {
  const username = workspace?.me?.username;
  if (!account || username === undefined) throw new Error('no account to sign in to');
  await account.signIn(username, password);
  location.reload();
}
