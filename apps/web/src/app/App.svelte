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
  `/org/<organization>` is the screen of the organization's administrators
  (`OrgPage`), in place of the workspace; the way to it is in the sidebar of
  whoever administers one. Only the server has organizations.
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import { setFileUrlResolver } from '$lib/editor/files.ts';
  import InvitePage from '$lib/features/account/InvitePage.svelte';
  import LinkedSignIns from '$lib/features/account/LinkedSignIns.svelte';
  import SignInNotice from '$lib/features/account/SignInNotice.svelte';
  import SignInPage from '$lib/features/account/SignInPage.svelte';
  import SsoRequired from '$lib/features/account/SsoRequired.svelte';
  import OrgPage from '$lib/features/organization/OrgPage.svelte';
  import AppShell from '$lib/features/shell/AppShell.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    BackendError,
    isBackendError,
    isSignedOut,
    providePeople,
    whileHere,
    Workspace,
    type AcceptedInvitation,
    type Account,
    type LinkedIdentity,
    type NewAccount,
    type Registration,
  } from '$lib/state/index.ts';
  import { userDisplayName } from '$lib/logic/users.ts';
  import type { BoardPlace } from '$lib/logic/urls.ts';
  import { OrgAdmin } from '$lib/state/org-admin.svelte.ts';
  import type { Organizations } from '$lib/state/organization.ts';
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
  import { keepReturn, takeReturn } from './return-to.ts';
  import { parseRoute, type Router } from './router.svelte.ts';
  import { signInOrganizations } from './sign-in-organizations.ts';

  let { router }: { router: Router } = $props();

  let workspace = $state.raw<Workspace | undefined>(undefined);
  let account = $state.raw<Account | undefined>(undefined);
  let organizations = $state.raw<Organizations | undefined>(undefined);
  /** The organization the reader administers, once asked; the sidebar then leads to it. */
  let administered = $state<string | undefined>(undefined);
  /**
   * The names of the account just made on this page: its first sign-in
   * copies the welcome board. Another account signed in here gets none.
   */
  let registered: { username: string; email: string } | undefined;

  /** What the address says of a sign-in through a provider that just came back. */
  const cameBack = new URLSearchParams(location.search);
  /** A provider was connected to the account just now: the settings say so, until they close. */
  let justLinked = $state(cameBack.has('oidc_linked'));
  let settingsOpen = $state(cameBack.has('oidc_linked'));
  /** The settings were opened for the connected sign-ins: that part comes into view. */
  let linkedAsked = $state(cameBack.has('oidc_linked'));
  /**
   * A sign-in through a provider came back refused (`/login?oidc_error=…`)
   * while the reader is signed in: told in the page, until they go on.
   */
  let refusal = $state(cameBack.get('oidc_error'));
  const refused = $derived(router.route.unknown ? refusal : null);

  function openLinked(): void {
    linkedAsked = true;
    settingsOpen = true;
  }

  /**
   * Back from a provider, the browser is at the root: the reader goes on to
   * where they were when they left for it, if that is a page of the app.
   */
  function resume(): void {
    const kept = takeReturn();
    if (cameBack.has('oidc_linked')) history.replaceState(null, '', location.pathname);
    const { unknown, workspaceId, orgId: organization } = router.route;
    if (kept === undefined || unknown || workspaceId || organization) return;
    const route = parseRoute(kept, router.basePath);
    if (route.invitation !== undefined) location.replace(router.inviteHref(route.invitation));
    else if (!route.unknown) router.go(route, { replace: true });
  }

  /** A session that is over is signing out, whatever was being asked: told, and passed on. */
  async function signedIn<T>(doing: () => Promise<T>): Promise<T> {
    try {
      return await doing();
    } catch (error) {
      if (isSignedOut(error)) workspace?.report('save', error);
      throw error;
    }
  }

  /** Sends the browser to the provider of an organization, to come back to this address. */
  async function toProvider(orgId: string, linkPassword?: string): Promise<void> {
    if (!account) return;
    // Started by this page's own request; the browser then goes to the provider.
    const address = await account.startOidc(orgId, linkPassword);
    keepReturn(location.pathname);
    location.assign(address);
  }

  const unlink = (identity: LinkedIdentity, password: string): Promise<void> =>
    signedIn(() => account?.unlinkIdentity(identity, password) ?? Promise.resolve());

  onMount(() => {
    resume();
    void startBackend().then(async (started) => {
      const { backend } = started;
      account = started.account;
      organizations = started.organizations;
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
    // Where this account was going is not where the next one starts.
    takeReturn();
    router.restart({});
  }

  /** The token of the invitation the address is the link of, if it is one. */
  const invitation = $derived(router.route.invitation);

  // The root address: to the workspace the reader starts in, in its place.
  $effect(() => {
    if (workspace?.status !== 'ready' || router.route.unknown || router.route.workspaceId) return;
    if (invitation !== undefined) return;
    if (router.route.orgId) return;
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

  // Once signed in: is there an organization the reader administers? Asked once.
  let askedAdministered = false;
  $effect(() => {
    if (workspace?.status !== 'ready' || !organizations || askedAdministered) return;
    askedAdministered = true;
    organizations.administered().then(
      (orgId) => {
        administered = orgId;
      },
      () => {
        // Not told: the way to it is not shown.
      },
    );
  });

  /** The reader's workspaces that this session opens: where else to go from one it does not. */
  const openWorkspaces = $derived(
    (workspace?.workspaces ?? [])
      .filter((one) => !one.ssoRequired)
      .map((one) => ({ ...one, href: router.href({ workspaceId: one.id }) })),
  );

  /** The organization the address names; the screen's state is made anew only when it changes. */
  const orgId = $derived(router.route.orgId);
  const orgAdmin = $derived(
    organizations && orgId ? new OrgAdmin(organizations, orgId) : undefined,
  );
  $effect(() => {
    // Once the reader is signed in: before that the organization is not there for them.
    if (workspace?.status === 'ready') void orgAdmin?.load();
  });

  const openBoard = (board: BoardPlace): void => {
    router.openBoard(board);
  };
</script>

{#snippet accountSettings()}
  {#if account && workspace}
    {@const of = account}
    {@const mine = workspace.workspaces}
    <LinkedSignIns
      read={() => signedIn(() => of.identities())}
      organizations={() => signInOrganizations(mine, (orgId) => of.loginOptions(orgId))}
      onLink={(orgId: string, password: string) => signedIn(() => toProvider(orgId, password))}
      onUnlink={unlink}
      {justLinked}
      reveal={linkedAsked}
    />
  {/if}
{/snippet}

<!-- Back by the browser's "back" from a provider the reader left for: nothing is on its way any more. -->
<svelte:window
  onpageshow={(event: PageTransitionEvent) => {
    if (event.persisted) takeReturn();
  }}
/>

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
    onSso={(orgId: string) => toProvider(orgId)}
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
    onOidcStart={(orgId: string) => toProvider(orgId)}
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
{:else if router.route.orgId}
  {#if orgAdmin}
    {@const closedOrgId = orgAdmin.orgId}
    <OrgPage
      admin={orgAdmin}
      locale={workspace.locale}
      meId={workspace.me?.id}
      homeHref={router.href({})}
      workspaceHref={(workspaceId: string) => router.href({ workspaceId })}
    >
      {#snippet closed()}
        <!-- The settings are the shell's: connecting single sign-on is done from the app's own page. -->
        <SsoRequired
          name=""
          others={openWorkspaces}
          onContinue={() => toProvider(closedOrgId)}
          onOpenLinked={() => {
            router.go({});
            openLinked();
          }}
          onGo={(workspaceId: string) => {
            router.go({ workspaceId });
          }}
        />
      {/snippet}
    </OrgPage>
  {:else}
    <NotFound {router} />
  {/if}
{:else}
  <AppShell
    {workspace}
    orgHref={administered ? router.href({ orgId: administered }) : undefined}
    onSwitchWorkspace={(workspaceId: string) => {
      router.go({ workspaceId });
    }}
    readRecords={organizations
      ? (workspaceId: string) => organizations?.workspaceRecords(workspaceId) ?? Promise.resolve([])
      : undefined}
    boardId={router.route.boardId}
    boardHref={(board: BoardPlace) =>
      router.href({ workspaceId: board.workspaceId, boardId: board.id })}
    sharedHref={(boardId: string, readToken: string) => router.href({ boardId, readToken })}
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
            if (isSignedOut(error)) workspace?.report('save', error);
            throw error;
          }
        }
      : undefined}
    accountSettings={account ? accountSettings : undefined}
    bind:settingsOpen={
      () => settingsOpen,
      (open: boolean) => {
        settingsOpen = open;
        // Said once, and asked for once: the next opening is the reader's own.
        if (!open) [justLinked, linkedAsked] = [false, false];
      }
    }
  >
    {@const here = workspace.asked === router.route.workspaceId}
    {#if refused}
      <SignInNotice
        problem={refused}
        onGoOn={() => {
          refusal = null;
          router.go({}, { replace: true });
        }}
        onOpenLinked={openLinked}
      />
    {:else if router.route.unknown || (here && workspace.entered === 'missing')}
      <NotFound {router} />
    {:else if here && workspace.entered === 'sso' && workspace.closed?.orgId}
      {@const orgId = workspace.closed.orgId}
      {#key orgId}
        <SsoRequired
          name={workspace.closed.title}
          others={openWorkspaces}
          onContinue={() => toProvider(orgId)}
          onOpenLinked={openLinked}
          onGo={(workspaceId: string) => {
            router.go({ workspaceId });
          }}
        />
      {/key}
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
