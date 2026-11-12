<!--
  The app: starts the backend, loads the reader's workspace, and puts the
  screens together. Features do not import each other; this folder is where
  they meet (src/lib/README.md).

  The workspace shown is the one the address names. The root address names
  none: the reader is taken to the one they were last in on this browser if
  it is still theirs, else to their first, the address put in place of the
  root's (`Workspace.firstChoice`, `last-workspace.ts`).

  The link of an invitation (`/invite/<token>`) shows the page of the
  invitation, to whoever is signed in and to whoever is not; where it lets
  them in is then loaded afresh.
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import { setFileUrlResolver } from '$lib/editor/files.ts';
  import InvitePage from '$lib/features/account/InvitePage.svelte';
  import SignInPage from '$lib/features/account/SignInPage.svelte';
  import AppShell from '$lib/features/shell/AppShell.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    BackendError,
    isBackendError,
    providePeople,
    whileHere,
    Workspace,
    type AcceptedInvitation,
    type Account,
    type NewAccount,
    type Registration,
  } from '$lib/state/index.ts';
  import { userDisplayName } from '$lib/logic/users.ts';
  import type { BoardPlace } from '$lib/logic/urls.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { startBackend } from './backend.ts';
  import BoardPage from './BoardPage.svelte';
  import { lastWorkspace, rememberWorkspace } from './last-workspace.ts';
  import {
    browserLocale,
    currentLocale,
    followAccountLanguage,
    locales,
    shownLanguage,
  } from './locale.ts';
  import NotFound from './NotFound.svelte';
  import Problems from './Problems.svelte';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();

  let workspace = $state.raw<Workspace | undefined>(undefined);
  let account = $state.raw<Account | undefined>(undefined);
  /**
   * The names of the account just made on this page: its first sign-in
   * copies the welcome board. Another account signed in here gets none.
   */
  let registered: { username: string; email: string } | undefined;

  onMount(() => {
    void startBackend().then(async (started) => {
      const { backend } = started;
      account = started.account;
      const made = new Workspace({ backend, locale: currentLocale, lastWorkspace });
      if (started.people) {
        providePeople(made, {
          people: started.people,
          inviteLink: (token: string) => new URL(router.inviteHref(token), location.origin).href,
        });
      }
      // The bodies of cards name their images by file; the board that is open says where they are.
      setFileUrlResolver((fileId) =>
        made.board ? backend.fileUrl(made.board.boardId, fileId) : undefined,
      );
      workspace = made;
      // On the handle of the browser tests and the console too, where there is
      // one (the backend in memory): a test undoes through it.
      const dev = (window as unknown as { fvociDev?: object }).fvociDev;
      if (dev) Object.assign(dev, { workspace: made });
      await made.load(router.route.workspaceId);
      followLanguage(made);
    });
    return () => {
      workspace?.close();
    };
  });

  /**
   * The page in the language of the account, once its settings are read:
   * another device may have chosen it. A page that loads again for it says
   * so; nothing else is to be done on this one.
   */
  function followLanguage(loaded: Workspace): boolean {
    return loaded.status === 'ready' && followAccountLanguage(loaded.settings.language);
  }

  /** Reads the workspace again after a failure, and follows its language as the first load does. */
  async function retryLoad(): Promise<void> {
    if (!workspace) return;
    await workspace.load(router.route.workspaceId);
    followLanguage(workspace);
  }

  /**
   * Saves the language to the account and shows the page in it. A refusal
   * leaves the page as it is, and says so (`changeSettings`).
   */
  async function chooseLanguage(setting: string): Promise<void> {
    if (!workspace) return;
    await workspace.changeSettings({ language: setting });
    if (workspace.settings.language === setting) followAccountLanguage(setting);
  }

  /**
   * Signs out, and starts the page again at its root: nothing of the
   * account that was signed in stays in memory. When the server does not
   * end the session (no answer, a refusal), the page stays as it is and
   * says so; a session that was over already (401) counts as signed out.
   */
  async function signOut(): Promise<void> {
    try {
      await account?.signOut();
    } catch (error) {
      if (!isBackendError(error) || error.status !== 401) {
        workspace?.report('save', error);
        return;
      }
    }
    workspace?.close();
    router.restart({});
  }

  /** The token of the invitation the address is the link of, if it is one. */
  const invitation = $derived(router.route.invitation);

  // The root address: to the workspace the reader starts in, in its place.
  $effect(() => {
    if (workspace?.status !== 'ready' || router.route.unknown || router.route.workspaceId) return;
    if (invitation !== undefined) return;
    const chosen = workspace.firstChoice();
    if (chosen) router.go({ workspaceId: chosen }, { replace: true });
  });

  // Another workspace in the address (a link, back or forward): it is entered.
  $effect(() => {
    const asked = router.route.workspaceId;
    if (workspace?.status === 'ready' && asked && asked !== workspace.asked) {
      void workspace.enter(asked);
    }
  });

  // The workspace entered is the one to come back to on this browser.
  $effect(() => {
    if (workspace?.entered === 'ready' && workspace.me && workspace.info) {
      rememberWorkspace(workspace.me.id, workspace.info.id);
    }
  });

  /** The workspace the address names, entered and read. */
  const shown = $derived(
    workspace?.entered === 'ready' && workspace.info?.id === router.route.workspaceId
      ? workspace.info
      : undefined,
  );

  // Without a board in the address, show the first one of the sidebar.
  $effect(() => {
    if (!shown || !workspace || router.route.boardId) return;
    const first = workspace.sidebar.flatMap((entry) => entry.boards)[0];
    if (first) router.go({ workspaceId: shown.id, boardId: first.id }, { replace: true });
  });

  const openBoard = (board: BoardPlace): void => {
    router.openBoard(board);
  };
</script>

{#if !workspace || workspace.status === 'idle' || workspace.status === 'loading'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
      <p class="mt-2 text-muted-foreground" role="status">{m.app_loading()}</p>
    </div>
  </main>
{:else if invitation !== undefined && account}
  {@const signedInAs = workspace.status === 'ready' ? workspace.me : undefined}
  {@const accounts = account}
  <InvitePage
    token={invitation}
    signedInAs={signedInAs && userDisplayName(signedInAs)}
    preview={(token: string) => accounts.previewInvitation(token)}
    accept={(token: string, made?: NewAccount) => accounts.acceptInvitation(token, made)}
    onSignIn={(login: string, password: string) => accounts.signIn(login, password)}
    onJoined={(accepted: AcceptedInvitation) => {
      // Loaded afresh where the invitation let the reader in: what the page
      // has in memory is of whoever was, or was not, signed in before. In
      // place of the link, whose token is not to stay in the history.
      router.restart(
        { workspaceId: accepted.workspaceId, boardId: accepted.boardId },
        { replace: true },
      );
    }}
    onSessionEnded={() => {
      location.reload();
    }}
    homeHref={router.href({})}
  />
{:else if workspace.status === 'signedOut' && account}
  <SignInPage
    onSignIn={async (login: string, password: string) => {
      await account?.signIn(login, password);
      // A session that ended while working here: what is on the page (the
      // open board, undo, problems) was the old session's. Start afresh, as
      // signing out does: at the same address when it is the same person,
      // else at the root, where they are taken to a workspace of theirs.
      const before = workspace?.me;
      if (workspace && before) {
        const now = await workspace.backend.me().catch(() => undefined);
        if (now?.id === before.id) location.reload();
        else router.restart({});
        return;
      }
      // Signed in on an address that is no page of the app (`/login`, where a
      // sign-in through a provider comes back refused, or any other): the first.
      if (router.route.unknown) router.go({}, { replace: true });
      await workspace?.load(router.route.workspaceId);
      // Signed in, yet the next request is not: the browser did not keep the
      // session (a cookie for https on a page over http, or cookies blocked).
      if (workspace?.status === 'signedOut') {
        throw new BackendError(401, {
          reason: 'session_not_kept',
          detail: 'the session was not kept',
        });
      }
      // A new account starts on its own copy of the welcome board, made once.
      const made = registered;
      if (made && workspace?.status === 'ready') {
        registered = undefined;
        if (login === made.username || login === made.email) {
          const signedIn = workspace;
          const where = signedIn.info?.id;
          const welcome = await whileHere(signedIn, () => signedIn.onboard());
          // The page may be on it already (the first board is shown by itself).
          if (welcome && where && router.route.boardId !== welcome) {
            router.openBoard({ id: welcome, workspaceId: where });
          }
        }
      }
      // Last: the page may load again in the account's language.
      if (workspace) followLanguage(workspace);
    }}
    onRegister={async (registration: Registration) => {
      await account?.register(registration);
      registered = { username: registration.username, email: registration.email };
    }}
    optionsOf={(orgId: string | undefined) =>
      account?.loginOptions(orgId) ?? Promise.resolve({ registrationOpen: true })}
    onOidcStart={async (orgId: string) => {
      if (!account) return;
      // Started by this page's own request; the browser then goes to the provider.
      location.assign(await account.startOidc(orgId));
    }}
  />
{:else if workspace.status === 'failed' || workspace.status === 'signedOut'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-lg font-medium" role="alert">{m.app_load_failed()}</p>
      <Button class="mt-4 roomy:min-h-touch" onclick={() => void retryLoad()}>
        {m.app_retry()}
      </Button>
    </div>
  </main>
{:else}
  <AppShell
    {workspace}
    boardId={router.route.boardId}
    boardHref={(board: BoardPlace) =>
      router.href({ workspaceId: board.workspaceId, boardId: board.id })}
    onOpenBoard={openBoard}
    {locales}
    language={shownLanguage(workspace.settings.language)}
    autoLocale={browserLocale()}
    onChooseLanguage={(setting: string) => void chooseLanguage(setting)}
    onSignOut={account ? () => void signOut() : undefined}
    onChangePassword={account
      ? async (oldPassword: string, newPassword: string) => {
          try {
            await account?.changePassword(oldPassword, newPassword);
          } catch (error) {
            // The session was over: that is signing out, not a wrong password.
            if (isBackendError(error) && error.status === 401) workspace?.report('save', error);
            throw error;
          }
        }
      : undefined}
  >
    {@const here = workspace.asked === router.route.workspaceId}
    {#if router.route.unknown || (here && workspace.entered === 'missing')}
      <NotFound {router} />
    {:else if !router.route.workspaceId && workspace.workspaces.length === 0}
      <div class="grid h-full place-items-center p-6 text-center">
        <p class="text-muted-foreground">{m.app_no_boards()}</p>
      </div>
    {:else if here && workspace.entered === 'failed'}
      <div class="grid h-full place-items-center p-6 text-center" data-testid="enter-failed">
        <div>
          <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
          <Button
            class="mt-4 roomy:min-h-touch"
            onclick={() => {
              if (router.route.workspaceId) void workspace?.enter(router.route.workspaceId);
            }}>{m.app_retry()}</Button
          >
        </div>
      </div>
    {:else if !shown}
      <div class="grid h-full place-items-center p-6">
        <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
      </div>
    {:else if router.route.boardId}
      <BoardPage {workspace} {router} />
    {:else}
      <div class="grid h-full place-items-center p-6 text-center">
        <p class="text-muted-foreground">{m.app_no_boards()}</p>
      </div>
    {/if}
  </AppShell>
  <Problems {workspace} />
{/if}
