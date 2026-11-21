<!--
  Where the notice to set a password stands (`features/account/PasswordNotice`):
  at the foot of the pane, in the reader's own space, for an account that has
  no password, once something is made there, until it is closed for good. That
  it was closed is kept with the account (the client state), so that no other
  browser shows it again. Set, the password ends it: the account has one, and
  the own space takes only a session made with it, so the pane asks for it
  (`passwordSet`) where the space was.

  A session too old to vouch is signed in again through the provider of the
  first of the reader's organizations that has one, and comes back here.
-->
<script lang="ts">
  import Notice from '$lib/features/account/PasswordNotice.svelte';
  import { isOwnSpace, passwordSet, type Account, type Workspace } from '$lib/state/index.ts';

  import { signInOrganizations } from './sign-in-organizations.ts';

  interface Props {
    workspace: Workspace;
    account: Account;
    /** Sends the browser through the organization's provider, back to this page. */
    onProvider: (orgId: string) => Promise<void>;
  }

  let { workspace, account, onProvider }: Props = $props();

  /** The entry of the client state that says the notice was closed for good. */
  const DISMISSED = 'passwordNoticeDismissed';

  const asks = $derived(isOwnSpace(workspace.info) && workspace.me?.hasPassword === false);
  /** Whether it was closed for good, read once it could be shown; a failed reading shows it. */
  const dismissed = $derived(
    asks
      ? workspace.backend
          .screenState()
          .then((kept) => kept[DISMISSED] === '1')
          .catch(() => false)
      : undefined,
  );
  const made = $derived(workspace.containers.some((container) => !container.isTemplate));
  /** Closed in this page: for good, or once the password is set. */
  let closed = $state(false);

  function dismiss(): void {
    closed = true;
    void workspace.backend.patchScreenState({ [DISMISSED]: '1' }).catch(() => undefined);
  }

  /**
   * Set, the account has a password and its own space takes only a session made
   * with it, which this one was not: the notice goes, and the space asks for it.
   */
  async function setPassword(password: string): Promise<void> {
    await account.setPassword(password);
    passwordSet(workspace);
  }

  async function signInAgain(): Promise<void> {
    const [first] = await signInOrganizations(workspace.workspaces, (orgId) =>
      account.loginOptions(orgId),
    );
    if (!first) throw new Error('no provider of the reader');
    await onProvider(first.orgId);
  }
</script>

{#if dismissed && made && !closed}
  {#await dismissed then gone}
    {#if !gone}
      <!-- Where the header of a view folds, a board has its round "New" at the foot: above it. -->
      <div
        class="pointer-events-none absolute inset-x-0 bottom-0 z-20 flex justify-center p-3 pb-[calc(5.5rem+var(--safe-bottom))] @tabs/pane:pb-[calc(0.75rem+var(--safe-bottom))]"
      >
        <div class="pointer-events-auto w-full max-w-md">
          <Notice
            username={workspace.me?.username ?? ''}
            onSet={(password: string) => setPassword(password)}
            onSignInAgain={signInAgain}
            onDismiss={dismiss}
            onClose={() => {
              closed = true;
            }}
          />
        </div>
      </div>
    {/if}
  {/await}
{/if}
